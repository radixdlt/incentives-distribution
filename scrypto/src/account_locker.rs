use scrypto::prelude::*;

/// Helper function to combine two AccessRules using OR logic.
///
/// This function properly handles all AccessRule variants and combines them
/// so that either rule will grant access.
fn combine_access_rules(rule1: AccessRule, rule2: AccessRule) -> AccessRule {
    match (rule1, rule2) {
        // If either is AllowAll, result is AllowAll
        (AccessRule::AllowAll, _) | (_, AccessRule::AllowAll) => AccessRule::AllowAll,
        // If one is DenyAll, use the other
        (AccessRule::DenyAll, other) | (other, AccessRule::DenyAll) => other,
        // Both are Protected, combine with AnyOf
        (AccessRule::Protected(req1), AccessRule::Protected(req2)) => {
            AccessRule::Protected(CompositeRequirement::AnyOf(vec![req1, req2]))
        }
    }
}

#[blueprint]
mod account_locker_blueprint {
    enable_method_auth! {
        methods {
            get_locker => PUBLIC;
            add_recoverer_rule => restrict_to: [OWNER];
            add_storer_rule => restrict_to: [OWNER];
            set_recoverer_role => restrict_to: [OWNER];
            set_storer_role => restrict_to: [OWNER];
        }
    }

    /// A simple wrapper blueprint around the native AccountLocker component.
    ///
    /// This blueprint exists to allow instantiation of AccountLocker components
    /// via cross-blueprint calls from other blueprints in this package. By wrapping
    /// the native AccountLocker in a blueprint, other components can create new
    /// AccountLocker instances through the standard blueprint instantiation pattern.
    ///
    /// The wrapper is minimal and simply stores a reference to the underlying
    /// AccountLocker component and provides access to it.
    struct AccountLockerWrapper {
        /// The underlying native AccountLocker component that this wrapper manages.
        locker: Global<AccountLocker>,
    }

    impl AccountLockerWrapper {
        /// Instantiates a new AccountLocker component and wraps it.
        ///
        /// This method creates a new native AccountLocker component with the
        /// specified access rules and returns both the wrapper component and
        /// a direct reference to the underlying AccountLocker for convenience.
        ///
        /// The final owner access rule will allow access by:
        /// 1. Anyone satisfying the provided `owner_access_rule`
        /// 2. The AccountLockerWrapper component itself (via global_caller)
        ///
        /// # Arguments
        ///
        /// - `owner_access_rule`: [`AccessRule`] - The access rule defining
        ///   who can act as owner of the created AccountLocker. This is typically
        ///   a rule requiring an owner badge and/or allowing the instantiating
        ///   component to access the locker.
        ///
        /// # Returns
        ///
        /// - `(Global<AccountLockerWrapper>, Global<AccountLocker>)` - A tuple
        ///   containing the wrapper component and a direct reference to the
        ///   underlying AccountLocker component.
        pub fn instantiate(
            owner_access_rule: AccessRule,
        ) -> (Global<AccountLockerWrapper>, Global<AccountLocker>) {
            let (address_reservation, component_address) =
                Runtime::allocate_component_address(AccountLockerWrapper::blueprint_id());

            // Combine the provided access rule with the wrapper's own component address
            let wrapper_access_rule = rule!(require(global_caller(component_address)));
            let combined_owner_rule = super::combine_access_rules(owner_access_rule, wrapper_access_rule);
            let owner_role = OwnerRole::Fixed(combined_owner_rule.clone());

            let locker = Blueprint::<AccountLocker>::instantiate(
                owner_role.clone(),
                combined_owner_rule.clone(),
                combined_owner_rule.clone(),
                combined_owner_rule.clone(),
                combined_owner_rule.clone(),
                None,
            );

            let wrapper = Self { locker }
                .instantiate()
                .prepare_to_globalize(owner_role)
                .with_address(address_reservation)
                .globalize();

            (wrapper, locker)
        }

        /// Returns a reference to the underlying AccountLocker component.
        ///
        /// This method provides access to the wrapped AccountLocker component
        /// so it can be used by other components or blueprints.
        ///
        /// # Returns
        ///
        /// - [`Global<AccountLocker>`] - A global reference to the underlying
        ///   AccountLocker component.
        pub fn get_locker(&self) -> Global<AccountLocker> {
            self.locker
        }

        /// Adds a new access rule to the existing "recoverer" role of the AccountLocker.
        ///
        /// This method retrieves the current recoverer role, combines it with the
        /// new rule using OR logic (allowing either the existing rules OR the new
        /// rule), and sets the updated role back on the AccountLocker.
        ///
        /// # Arguments
        ///
        /// - `new_rule`: [`AccessRule`] - The additional access rule to add to
        ///   the recoverer role.
        pub fn add_recoverer_rule(&self, new_rule: AccessRule) {
            let current_role = self.locker.get_role("recoverer");
            let updated_rule = match current_role {
                Some(existing_rule) => super::combine_access_rules(existing_rule, new_rule),
                None => new_rule,
            };
            self.locker.set_role("recoverer", updated_rule);
        }

        /// Adds a new access rule to the existing "storer" role of the AccountLocker.
        ///
        /// This method retrieves the current storer role, combines it with the
        /// new rule using OR logic (allowing either the existing rules OR the new
        /// rule), and sets the updated role back on the AccountLocker.
        ///
        /// # Arguments
        ///
        /// - `new_rule`: [`AccessRule`] - The additional access rule to add to
        ///   the storer role.
        pub fn add_storer_rule(&self, new_rule: AccessRule) {
            let current_role = self.locker.get_role("storer");
            let updated_rule = match current_role {
                Some(existing_rule) => super::combine_access_rules(existing_rule, new_rule),
                None => new_rule,
            };
            self.locker.set_role("storer", updated_rule);
        }

        /// Completely replaces the "recoverer" role of the AccountLocker.
        ///
        /// This method overwrites the existing recoverer role with the provided
        /// access rule, discarding any previous rules.
        ///
        /// # Arguments
        ///
        /// - `new_rule`: [`AccessRule`] - The new access rule to set as the
        ///   recoverer role, replacing all existing rules.
        pub fn set_recoverer_role(&self, new_rule: AccessRule) {
            self.locker.set_role("recoverer", new_rule);
        }

        /// Completely replaces the "storer" role of the AccountLocker.
        ///
        /// This method overwrites the existing storer role with the provided
        /// access rule, discarding any previous rules.
        ///
        /// # Arguments
        ///
        /// - `new_rule`: [`AccessRule`] - The new access rule to set as the
        ///   storer role, replacing all existing rules.
        pub fn set_storer_role(&self, new_rule: AccessRule) {
            self.locker.set_role("storer", new_rule);
        }
    }
}
