use std::collections::HashMap;

use strum::{EnumIter, IntoEnumIterator};
use wasmtime::{Caller, Engine, Func, Instance, Module, Store};

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
}

struct UserData {
    balance: MoneyUnit,
    hosting_days_left: u32,
}

struct State {
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

fn instantiate_services_management_module(
    mut store: &mut SMStore,
    user: UserId,
    module: &Module,
) -> Result<Instance, wasmtime::Error> {
    let balance_fn = Func::wrap(&mut store, move |caller: Caller<'_, State>| {
        caller.data().user_data[&user].balance.to_cents_as_i64()
    });
    let order_hosting_fn = Func::wrap(
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
    );
    let instance = Instance::new(
        store,
        &module,
        &[balance_fn.into(), order_hosting_fn.into()],
    )?;
    Ok(instance)
}

fn main() {
    let engine = Engine::default();
    let wat = r#"
        (module
            (import "host" "host_func" (func $balance (result i64)))
            (import "host" "host_func" (func $order_hosting (param i32) (result i32)))

            (func (export "run") (result i64)
                (i32.const 30)  ;; Pass 30 to order hosting in order to order a month of hosting
                (call $order_hosting)

                ;; Discard the error code
                (drop)

                (call $balance)
            )
        )
    "#;
    let module = Module::new(&engine, wat).unwrap();
    let mut store = {
        let mut user_data = HashMap::new();

        user_data.insert(
            UserId(0),
            UserData {
                balance: MoneyUnit::from_cents(1_000_00),
                hosting_days_left: 0,
            },
        );

        let data = State { user_data };
        Store::new(&engine, data)
    };
    let instance = instantiate_services_management_module(&mut store, UserId(0), &module).unwrap();
    let run_fn = instance
        .get_typed_func::<(), i64>(&mut store, "run")
        .unwrap();
    let balance = run_fn.call(&mut store, ()).unwrap();
    println!("The balance of root is {balance}");
}
