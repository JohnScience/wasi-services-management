use std::collections::HashMap;

use strum::{EnumIter, IntoEnumIterator};
use wasmtime::{Caller, Engine, Extern, Func, ImportType, Instance, Linker, Module, Store};
use wasmtime_wasi::{sync::WasiCtxBuilder, WasiCtx};

mod money;

use money::MoneyUnit;

#[derive(Debug, thiserror::Error, EnumIter)]
enum Error {
    #[error("Invalid argument value passed to the function.")]
    InvalidArgumentValue,
    #[error("The total cost of the service exceeded the maximum value.")]
    TotalCostExceededMaxValue,
    #[error("The balance is negative.")]
    NegativeBalance,
    #[error("The balance would become negative after the transaction.")]
    BalanceWouldBecomeNegative,
    #[error("The balance would underflow after the transaction.")]
    BalanceWouldUnderflow,
    #[error("The requested import (e.g. a host function) is unknown.")]
    UnknownImport,
}

struct UserData {
    balance: MoneyUnit,
    hosting_days_left: u32,
}

struct State {
    wasi_ctx: WasiCtx,
    user_data: HashMap<UserId, UserData>,
}

type SMStore = Store<State>;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct UserId(usize);

fn order_hosting(user_data: &mut UserData, days: i32) -> Result<(), Error> {
    const PRICE_PER_DAY: MoneyUnit = MoneyUnit::from_cents(100);
    if days <= 0 {
        return Err(Error::InvalidArgumentValue);
    };

    let total_cost = (PRICE_PER_DAY * days).ok_or(Error::TotalCostExceededMaxValue)?;
    user_data.balance = (user_data.balance - total_cost)?;
    user_data.hosting_days_left += days as u32;
    Ok(())
}

fn resolve_or_construct_import<'a>(
    linker: &Linker<State>,
    mut store: &mut Store<State>,
    import: ImportType<'a>,
    user: UserId,
) -> Option<Extern> {
    if import.module() != "host" {
        return linker.get_by_import(&mut store, &import);
    };

    let host_import = match import.name() {
        "balance" => Func::wrap(&mut store, move |caller: Caller<'_, State>| {
            caller.data().user_data[&user].balance.to_cents_as_i64()
        }),
        "order_hosting" => Func::wrap(
            &mut store,
            move |mut caller: Caller<'_, State>, days: i32| {
                let user_data = caller.data_mut().user_data.get_mut(&user).unwrap();
                let ret = match order_hosting(user_data, days) {
                    Ok(()) => 0,
                    Err(e) => {
                        let discr = std::mem::discriminant(&e);
                        let error_code = Error::iter()
                            .map(|err| core::mem::discriminant(&err))
                            .enumerate()
                            .find_map(|(i, d)| if d == discr { Some(i + 1) } else { None });
                        match error_code {
                            Some(error_code) => {
                                debug_assert!(error_code > 0);
                                error_code
                            }
                            None => unreachable!(),
                        }
                    }
                };
                ret as i32
            },
        ),
        _ => return None,
    };
    Some(Extern::Func(host_import))
}

fn instantiate_services_management_module(
    linker: &Linker<State>,
    store: &mut SMStore,
    user: UserId,
    module: &Module,
) -> Result<Instance, Error> {
    let imports = module
        .imports()
        .map(|import| resolve_or_construct_import(linker, store, import, user))
        .collect::<Option<Vec<Extern>>>()
        .ok_or(Error::UnknownImport)?;
    let instance = Instance::new(store, &module, &imports).unwrap();
    Ok(instance)
}

fn main() {
    let engine = Engine::default();
    let wat = r#"
        (module
            (import "host" "balance" (func $balance (result i64)))
            (import "host" "order_hosting" (func $order_hosting (param i32) (result i32)))

            (func (export "run") (result i64)
                (i32.const 30)  ;; Pass 30 to $order_hosting in order to order a month of hosting
                (call $order_hosting)

                ;; Discard the error code
                (drop)

                (call $balance)
            )
        )
    "#;
    let mut linker = Linker::<State>::new(&engine);

    wasmtime_wasi::add_to_linker(&mut linker, |s| &mut s.wasi_ctx).unwrap();
    let mut store = {
        let mut user_data = HashMap::new();

        user_data.insert(
            UserId(0),
            UserData {
                balance: MoneyUnit::from_cents(1_000_00),
                hosting_days_left: 0,
            },
        );

        let wasi_ctx = WasiCtxBuilder::new().inherit_stdio().build();

        let data = State {
            user_data,
            wasi_ctx,
        };
        Store::new(&engine, data)
    };

    let module = Module::new(&engine, wat).unwrap();
    // let instance = linker.instantiate(&mut store, &module).unwrap();
    let instance =
        instantiate_services_management_module(&linker, &mut store, UserId(0), &module).unwrap();
    let run_fn = instance
        .get_typed_func::<(), i64>(&mut store, "run")
        .unwrap();
    let balance = run_fn.call(&mut store, ()).unwrap();
    println!("The balance of root is {balance}");
}
