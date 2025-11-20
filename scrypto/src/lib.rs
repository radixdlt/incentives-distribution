use scrypto::prelude::*;

#[blueprint]
mod incentives_vester {

    enable_method_auth! {
        methods {
            refill => PUBLIC;
            redeem => PUBLIC;
            get_maturity_value => PUBLIC;
            claim => restrict_to: [OWNER];
            remove_lp => restrict_to: [OWNER];
        }
    }

    struct IncentivesVester {
        locker: Global<AccountLocker>,
        pool: Global<OneResourcePool>,
        claimed_users: KeyValueStore<String, ()>,

        // LP tokens that represent the user's share of the pool
        lp_tokens_vault: FungibleVault,

        // Tokens that are still locked (NOT yet vested into the pool)
        locked_tokens_vault: FungibleVault,

        // Total amount of tokens that went into this vesting schedule
        initial_token_input: Decimal,

        // How many tokens have already vested (i.e. have been moved into the pool)
        vested_tokens: Decimal,

        vest_start: Instant,
        vest_end: Instant,

        // Fraction [0, 1] of tokens that are vested immediately at start
        initial_vested_fraction: Decimal,
    }

    impl IncentivesVester {
        pub fn instantiate(
            admin_badge: FungibleBucket,
            vest_start: Instant,
            vest_end: Instant,
            initial_vested_fraction: Decimal,
            tokens_to_vest: FungibleBucket,
            dapp_def_address: ComponentAddress,
        ) -> (Global<IncentivesVester>, FungibleBucket) {
            let (address_reservation, component_address) =
                Runtime::allocate_component_address(IncentivesVester::blueprint_id());

            assert!(
                vest_end > vest_start,
                "vest_end must be strictly after vest_start"
            );
            assert!(
                initial_vested_fraction >= Decimal::ZERO && initial_vested_fraction <= Decimal::ONE,
                "initial_vested_fraction must be between 0 and 1"
            );

            let initial_token_input = tokens_to_vest.amount();
            assert!(
                initial_token_input > Decimal::ZERO,
                "tokens_to_vest must be greater than zero"
            );

            let admin_badge_address = admin_badge.resource_address();
            let admin_access_rule =
                rule!(require(admin_badge_address) || require(global_caller(component_address)));
            let admin_owner_role = OwnerRole::Fixed(admin_access_rule.clone());

            let locker = Blueprint::<AccountLocker>::instantiate(
                admin_owner_role.clone(),
                admin_access_rule.clone(),
                admin_access_rule.clone(),
                admin_access_rule.clone(),
                admin_access_rule.clone(),
                None,
            );

            let mut pool = Blueprint::<OneResourcePool>::instantiate(
                admin_owner_role.clone(),
                admin_access_rule,
                tokens_to_vest.resource_address(),
                None,
            );

            let (lp_tokens, unvested_tokens) = admin_badge.authorize_with_all(|| {
                let lp_tokens = pool.contribute(tokens_to_vest);

                let unvested_tokens = pool.protected_withdraw(
                    initial_token_input * (Decimal::ONE - initial_vested_fraction),
                    WithdrawStrategy::Rounded(RoundingMode::ToZero),
                );

                (lp_tokens, unvested_tokens)
            });

            let component = Self {
                locker,
                pool,
                lp_tokens_vault: FungibleVault::with_bucket(lp_tokens),

                // Remaining tokens are locked and will vest over time
                locked_tokens_vault: FungibleVault::with_bucket(unvested_tokens),

                initial_token_input,

                // Already vested amount = initial immediate vest
                vested_tokens: initial_token_input * initial_vested_fraction,

                vest_start,
                vest_end,
                initial_vested_fraction,
                claimed_users: KeyValueStore::new(),
            }
            .instantiate()
            .prepare_to_globalize(admin_owner_role)
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "Incentives Vester".to_string(), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize();

            (component, admin_badge)
        }

        pub fn refill(&mut self) {
            let current_time = Clock::current_time_rounded_to_seconds();

            let vest_duration =
                self.vest_end.seconds_since_unix_epoch - self.vest_start.seconds_since_unix_epoch;

            let elapsed =
                current_time.seconds_since_unix_epoch - self.vest_start.seconds_since_unix_epoch;

            let raw_progress = Decimal::from(elapsed) / Decimal::from(vest_duration);

            let vest_progress = if raw_progress <= Decimal::ZERO {
                Decimal::ZERO
            } else if raw_progress >= Decimal::ONE {
                Decimal::ONE
            } else {
                raw_progress
            };

            // Target total vested amount at this point in time
            let vested_tokens_target = self.initial_token_input
                * (self.initial_vested_fraction
                    + (Decimal::ONE - self.initial_vested_fraction) * vest_progress);

            let tokens_to_vest_now = vested_tokens_target - self.vested_tokens;

            if tokens_to_vest_now <= Decimal::ZERO {
                return;
            }

            let tokens = self.locked_tokens_vault.take(tokens_to_vest_now);
            self.pool.protected_deposit(tokens);

            self.vested_tokens = vested_tokens_target;
        }

        pub fn redeem(&mut self, lp_token_bucket: FungibleBucket) -> FungibleBucket {
            assert!(
                lp_token_bucket.amount() > Decimal::ZERO,
                "LP bucket must contain some amount"
            );
            self.refill();
            self.pool.redeem(lp_token_bucket)
        }

        pub fn claim(
            &mut self,
            lp_token_amount: Decimal,
            user_id: String,
            account_address: Global<Account>,
        ) {
            assert!(
                lp_token_amount > Decimal::ZERO,
                "LP token amount must be greater than zero"
            );

            assert!(
                self.claimed_users.get(&user_id).is_none(),
                "User has already claimed LP tokens"
            );

            self.claimed_users.insert(user_id, ());

            let lp_tokens = self.lp_tokens_vault.take(lp_token_amount);
            self.locker.store(account_address, lp_tokens.into(), true);

            // Potentially, we can mint an NFT here to represent the user's performance in Season 1
            // We would also deposit it with the account_locker
        }

        pub fn remove_lp(&mut self) -> FungibleBucket {
            self.lp_tokens_vault.take_all()
        }

        /// Returns the projected value of 1 LP token at full maturity
        /// Panics when current redemption value is 0, but should be no issue
        pub fn get_maturity_value(&mut self) -> Decimal {
            self.refill();

            let current_redemption_value = self.pool.get_redemption_value(Decimal::ONE);

            let current_unlocked_amount = self.pool.get_vault_amount();
            let still_locked_amount = self.locked_tokens_vault.amount();

            let final_token_amount = current_unlocked_amount + still_locked_amount;

            let maturity_factor = final_token_amount / current_unlocked_amount;

            maturity_factor * current_redemption_value
        }
    }
}
