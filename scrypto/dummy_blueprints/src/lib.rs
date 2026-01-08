use scrypto::prelude::*;

#[blueprint]
mod dummy_account_blueprint {
    struct DummyAccount {
        account: Global<Account>,
    }

    impl DummyAccount {
        pub fn instantiate_account() -> (Global<DummyAccount>, Global<Account>) {
            let account =
                Blueprint::<Account>::create_advanced(OwnerRole::Fixed(rule!(allow_all)), None);

            let component = Self { account }
                .instantiate()
                .prepare_to_globalize(OwnerRole::Fixed(rule!(allow_all)))
                .globalize();

            (component, account)
        }

        pub fn balance(&self, address: ResourceAddress) -> Decimal {
            self.account.balance(address)
        }

        pub fn withdraw(&mut self, address: ResourceAddress, amount: Decimal) -> Bucket {
            self.account.withdraw(address, amount)
        }
    }
}

#[blueprint]
mod dummy_locker_blueprint {
    struct DummyLocker {
        locker: Global<AccountLocker>,
    }

    impl DummyLocker {
        pub fn instantiate_locker() -> (Global<DummyLocker>, Global<AccountLocker>) {
            let locker = Blueprint::<AccountLocker>::instantiate(
                OwnerRole::Fixed(rule!(allow_all)),
                rule!(allow_all),
                rule!(allow_all),
                rule!(allow_all),
                rule!(allow_all),
                None,
            );

            let component = Self { locker }
                .instantiate()
                .prepare_to_globalize(OwnerRole::Fixed(rule!(allow_all)))
                .globalize();

            (component, locker)
        }

        pub fn get_locker(&self) -> Global<AccountLocker> {
            self.locker
        }
    }
}
