use scrypto::prelude::*;

#[blueprint]
mod incentives_vester {

    enable_method_auth! {
        roles {
            super_admin => updatable_by: [];
            admin => updatable_by: [super_admin];
        },
        methods {
            refill => PUBLIC;
            redeem => PUBLIC;
            get_maturity_value => PUBLIC;
            get_lp_token_amount => PUBLIC;
            get_pool_vault_amount => PUBLIC;
            get_locked_vault_amount => PUBLIC;
            get_pool_unit_resource_address => PUBLIC;
            get_pool_redemption_value => PUBLIC;
            get_vested_tokens => PUBLIC;
            get_total_tokens_to_vest => PUBLIC;
            claim => restrict_to: [super_admin, admin];
            finish_setup => restrict_to: [super_admin];
            create_pool_units => restrict_to: [super_admin];
            remove_lp => restrict_to: [super_admin];
            put_lp => restrict_to: [super_admin];
            put_locked_tokens => restrict_to: [super_admin];
            remove_locked_tokens => restrict_to: [super_admin];
        }
    }

    struct IncentivesVester {
        locker: Global<AccountLocker>,
        pool: Global<OneResourcePool>,

        // LP tokens that represent the user's share of the pool
        lp_tokens_vault: FungibleVault,

        // Tokens that are still locked (NOT yet vested into the pool)
        locked_tokens_vault: FungibleVault,

        // How many tokens have already vested (i.e. have been moved into the pool)
        total_tokens_to_vest: Decimal,
        vested_tokens: Decimal,

        // Vest start and end are only set once the vesting has been started
        vest_start: Option<Instant>,
        vest_end: Option<Instant>,

        // Duration of the vest, in days
        vest_duration_days: i64,
        // Pre claim duration in seconds
        pre_claim_duration_seconds: i64,
        // Fraction [0, 1] of tokens that are vested immediately at start
        initial_vested_fraction: Decimal,
    }

    impl IncentivesVester {
        pub fn instantiate(
            admin_badge_address: ResourceAddress,
            super_admin_badge_address: ResourceAddress,
            vest_duration_days: i64,
            initial_vested_fraction: Decimal,
            pre_claim_duration_seconds: i64,
            token_to_vest: ResourceAddress,
            dapp_def_address: ComponentAddress,
        ) -> Global<IncentivesVester> {
            let (address_reservation, component_address) =
                Runtime::allocate_component_address(IncentivesVester::blueprint_id());

            assert!(vest_duration_days > 0, "Vest duration must be positive");
            assert!(
                initial_vested_fraction >= Decimal::ZERO && initial_vested_fraction <= Decimal::ONE,
                "initial_vested_fraction must be between 0 and 1"
            );
            assert!(
                pre_claim_duration_seconds >= 0,
                "Pre-claim period must not have negative duration."
            );

            let admin_access_rule = rule!(require(admin_badge_address));

            let super_admin_access_rule = rule!(
                require(super_admin_badge_address) || require(global_caller(component_address))
            );
            let super_admin_owner_role = OwnerRole::Fixed(super_admin_access_rule.clone());

            let locker = Blueprint::<AccountLocker>::instantiate(
                super_admin_owner_role.clone(),
                super_admin_access_rule.clone(),
                super_admin_access_rule.clone(),
                super_admin_access_rule.clone(),
                super_admin_access_rule.clone(),
                None,
            );

            let pool = Blueprint::<OneResourcePool>::instantiate(
                super_admin_owner_role.clone(),
                super_admin_access_rule,
                token_to_vest,
                None,
            );

            let pool_unit_global_address: GlobalAddress =
                pool.get_metadata("pool_unit").unwrap().unwrap();
            let pool_unit_resource_address =
                ResourceAddress::try_from(pool_unit_global_address).unwrap();

            // We can set the metadata of the pool unit here immediately.
            // But we would need to pass the super_admin_badge at instantiation to allow that.
            // Let's not for now.

            Self {
                locker,
                pool,

                // Vault that will hold the pool units the users can claim
                lp_tokens_vault: FungibleVault::new(pool_unit_resource_address),

                // Vault that will be filled with tokens to vest (that are still unvested)
                locked_tokens_vault: FungibleVault::new(token_to_vest),

                // Already vested amount = initial immediate vest
                vested_tokens: Decimal::ZERO,
                total_tokens_to_vest: Decimal::ZERO,

                // Vest will only start once all lp tokens have been created. This will them turn into a Some.
                vest_start: None,
                vest_end: None,

                // Vesting parameters

                // Duration of vest in days
                vest_duration_days,
                // Pre-claim duration in seconds
                pre_claim_duration_seconds,
                // Amount of tokens users can immediately access from the start of the vest.
                initial_vested_fraction,
            }
            .instantiate()
            .prepare_to_globalize(super_admin_owner_role)
            .roles(roles! {
                super_admin => OWNER;
                admin => admin_access_rule;
            })
            .with_address(address_reservation)
            .metadata(metadata! {
                init {
                    "name" => "Incentives Vester".to_string(), updatable;
                    "dapp_definition" => dapp_def_address, updatable;
                }
            })
            .globalize()
        }

        pub fn create_pool_units(&mut self, tokens_to_vest: FungibleBucket) {
            assert!(self.vest_start.is_none(), "Vesting has already started");

            // Track the actual amount of tokens contributed
            let amount = tokens_to_vest.amount();
            self.total_tokens_to_vest += amount;

            let lp_tokens = self.pool.contribute(tokens_to_vest);
            self.lp_tokens_vault.put(lp_tokens);
        }

        pub fn finish_setup(&mut self) {
            assert!(self.vest_start.is_none(), "Vesting has already started");

            let current_time = Clock::current_time_rounded_to_seconds();
            let pre_claim_end = current_time
                .add_seconds(self.pre_claim_duration_seconds)
                .unwrap();

            self.vest_start = Some(pre_claim_end);
            self.vest_end = Some(pre_claim_end.add_days(self.vest_duration_days).unwrap());

            let tokens_to_unvest = self.pool.get_vault_amount();

            let unvested_tokens = self.pool.protected_withdraw(
                tokens_to_unvest,
                WithdrawStrategy::Rounded(RoundingMode::ToZero),
            );

            self.locked_tokens_vault.put(unvested_tokens);
        }

        pub fn refill(&mut self) {
            if let Some(vest_start) = self.vest_start {
                assert!(
                    Clock::current_time_is_at_or_after(vest_start, TimePrecision::Second),
                    "Still in pre-claim period. Vesting not started yet."
                );
            } else {
                panic!("Vesting setup not complete yet.");
            }

            let current_time = Clock::current_time_rounded_to_seconds();

            let vest_duration = self.vest_end.unwrap().seconds_since_unix_epoch
                - self.vest_start.unwrap().seconds_since_unix_epoch;

            let elapsed = current_time.seconds_since_unix_epoch
                - self.vest_start.unwrap().seconds_since_unix_epoch;

            let raw_progress = Decimal::from(elapsed) / Decimal::from(vest_duration);

            let vest_progress = if raw_progress <= Decimal::ZERO {
                Decimal::ZERO
            } else if raw_progress >= Decimal::ONE {
                Decimal::ONE
            } else {
                raw_progress
            };

            // Apply initial vested fraction + linear vesting of the remainder
            // At vest_start (progress = 0): initial_vested_fraction is available
            // At vest_end (progress = 1): 100% is available
            // Formula: initial + (1 - initial) * progress
            let vested_fraction = self.initial_vested_fraction
                + (Decimal::ONE - self.initial_vested_fraction) * vest_progress;

            // Target total vested amount at this point in time
            let vested_tokens_target = self.total_tokens_to_vest * vested_fraction;

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

        pub fn claim(&mut self, lp_token_amount: Decimal, account_address: Global<Account>) {
            assert!(self.vest_start.is_some(), "Vesting not set up yet.");

            assert!(
                lp_token_amount > Decimal::ZERO,
                "LP token amount must be greater than zero"
            );

            let lp_tokens = self.lp_tokens_vault.take(lp_token_amount);
            self.locker.store(account_address, lp_tokens.into(), true);

            // Potentially, we can mint an NFT here to represent the user's performance in Season 1
            // We would also deposit it with the account_locker
        }

        pub fn remove_lp(&mut self) -> FungibleBucket {
            self.lp_tokens_vault.take_all()
        }

        pub fn put_lp(&mut self, tokens: FungibleBucket) {
            self.lp_tokens_vault.put(tokens)
        }

        pub fn remove_locked_tokens(&mut self) -> FungibleBucket {
            self.locked_tokens_vault.take_all()
        }

        pub fn put_locked_tokens(&mut self, tokens: FungibleBucket) {
            self.locked_tokens_vault.put(tokens)
        }

        pub fn get_lp_token_amount(&mut self) -> Decimal {
            self.lp_tokens_vault.amount()
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

        pub fn get_pool_vault_amount(&mut self) -> Decimal {
            self.pool.get_vault_amount()
        }

        pub fn get_locked_vault_amount(&mut self) -> Decimal {
            self.locked_tokens_vault.amount()
        }

        pub fn get_pool_unit_resource_address(&self) -> ResourceAddress {
            self.lp_tokens_vault.resource_address()
        }

        pub fn get_pool_redemption_value(&self, lp_amount: Decimal) -> Decimal {
            self.pool.get_redemption_value(lp_amount)
        }

        pub fn get_vested_tokens(&self) -> Decimal {
            self.vested_tokens
        }

        pub fn get_total_tokens_to_vest(&self) -> Decimal {
            self.total_tokens_to_vest
        }
    }
}
