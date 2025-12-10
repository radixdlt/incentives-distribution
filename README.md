# Incentives Vester Blueprint
This blueprint has been deployed on Stokenet for testing at package address: `package_rdx1phe8ngw6fjahenrg9l5z548ve7u7z60a0pq98vkh6p2wf253xd6uh0`

## How this works

The vester distributes tokens to users over time through a vesting schedule. Here's the flow:

1. **Setup phase** - Admin creates the component, fills it with tokens, and calls `finish_setup`. This starts the pre-claim period.

2. **Pre-claim period** - A countdown period (e.g., 7 days) where LP tokens are distributed to user accounts. When this is triggered, tokens are removed from the pool and vesting doesn't start yet. Users can't redeem their LP tokens yet. This protects us from potential attacks, and reduces the impact of them.

3. **Vesting period** - After the pre-claim period ends, vesting begins. Tokens gradually unlock over the configured duration (e.g., 1 year). An initial fraction (e.g., 20%) is immediately available. The rest unlocks linearly over time.

4. **Redemption** - Users can redeem their LP tokens at any time during vesting. They receive the vested portion and forfeit the unvested portion. For example, if 50% has vested, redeeming gives 50% of tokens and forfeits the other 50%. The forfeited portion goes to the users that still haven't redeemed.

The `refill` method moves vested tokens from the locked vault into the pool, updating LP token values. This happens automatically during redemption but can be called manually to show accurate values in wallets.

## Admin badges
The component uses two types of badges:
- **Super admin badge** - Can perform all admin operations (creating pool units, finishing setup, removing LP/locked tokens)
- **Admin badge** - Can only claim LP tokens for users (held by backend)

## Setup sequence

### 1. Instantiate the component
Create the vester with basic parameters. No tokens required yet.

Parameters:
- `admin_badge_address` - Address of the admin badge (for backend claiming)
- `super_admin_badge_address` - Address of the super admin badge
- `vest_duration_days` - How many days the vest lasts (e.g., `30i64` for 30 days)
- `initial_vested_fraction` - Fraction immediately accessible (e.g., `Decimal("0.2")` for 20%)
- `pre_claim_duration_seconds` - Pre-claim period in seconds (e.g., `86400i64` for 1 day)
- `token_to_vest` - Resource address of token to vest (e.g., XRD)
- `dapp_definition_address` - Dapp definition address (you don't need to care about this when testing)

Instantiation manifest:
```
CALL_FUNCTION
  Address("package_tdx_2_1pk03fls3pdjf5dewt0kewhpx9syyj5vd4wq808sffcq5ghjk7svd4y")
  "IncentivesVester"
  "instantiate"
  Address("{admin_badge_address}") # admin badge for backend, create yourself in advance
  Address("{super_admin_badge_address}") # super admin badge, create yourself in advance
  30i64 # vest duration in days
  Decimal("0.2") # initial vested fraction (20%)
  86400i64 # pre-claim period in seconds (1 day)
  Address("resource_tdx_2_1tknxxxxxxxxxradxrdxxxxxxxxx009923554798xxxxxxxxxtfd2jc") # XRD
  Address("{dapp_definition_address}") # No need to care about this when testing
;

CALL_METHOD
  Address("{your_account_address}")
  "deposit_batch"
  Expression("ENTIRE_WORKTOP")
;
```

### 2. Fill the pool with tokens
Add tokens to create LP tokens. Can be done multiple times before finishing setup.

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_super_admin_badge}")
  "create_proof_of_amount"
  Address("{super_admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{your_account_address}")
  "withdraw"
  Address("resource_tdx_2_1tknxxxxxxxxxradxrdxxxxxxxxx009923554798xxxxxxxxxtfd2jc") # XRD
  Decimal("10000")
;

TAKE_ALL_FROM_WORKTOP
  Address("resource_tdx_2_1tknxxxxxxxxxradxrdxxxxxxxxx009923554798xxxxxxxxxtfd2jc")
  Bucket("rewards")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "create_pool_units"
  Bucket("rewards")
;
```

### 3. Finish setup (starts pre-claim period)
This removes tokens from the pool and starts the pre-claim countdown. After the pre-claim period ends, vesting begins.

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_super_admin_badge}")
  "create_proof_of_amount"
  Address("{super_admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "finish_setup"
;
```

## Claiming LP
During the pre-claim period, LP tokens can be claimed and sent to user accounts. The backend holds the admin badge to perform this operation.

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_admin_badge}")
  "create_proof_of_amount"
  Address("{admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "claim"
  Decimal("{amount_of_lp_tokens_to_distribute}")
  Address("{user_account_address}")
;
```

## Redeem
After the pre-claim period ends and vesting begins, users can redeem their LP tokens for the vested portion of tokens. The unvested portion is forfeited.

Manifest:
```
CALL_METHOD
  Address("{user_account}")
  "withdraw"
  Address("{lp_token_address}")
  Decimal("{amount_to_redeem}")
;

TAKE_ALL_FROM_WORKTOP
  Address("{lp_token_address}")
  Bucket("lp_tokens")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "redeem"
  Bucket("lp_tokens")
;

CALL_METHOD
  Address("{user_account}")
  "deposit_batch"
  Expression("ENTIRE_WORKTOP")
;
```

## Refill
Tokens vest over time but aren't automatically moved into the pool. Call `refill` to update the pool with vested tokens. This is automatically called during redemption, but can be called manually to show accurate LP token value in wallets.

Manifest:
```
CALL_METHOD
  Address("{incentives_vester_component_address}")
  "refill"
;
```

## Super Admin Operations
These methods allow the super admin to withdraw tokens from the smart contract. Use these with extreme caution as they can affect user balances.

### Remove LP Tokens
Withdraws all LP tokens from the component's internal vault. This does NOT affect LP tokens already claimed by users.

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_super_admin_badge}")
  "create_proof_of_amount"
  Address("{super_admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "remove_lp"
;

CALL_METHOD
  Address("{your_account_address}")
  "deposit_batch"
  Expression("ENTIRE_WORKTOP")
;
```

### Remove Locked Tokens
Withdraws all locked (unvested) tokens from the component. This will affect future vesting.

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_super_admin_badge}")
  "create_proof_of_amount"
  Address("{super_admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "remove_locked_tokens"
;

CALL_METHOD
  Address("{your_account_address}")
  "deposit_batch"
  Expression("ENTIRE_WORKTOP")
;
```

### Withdraw from Pool
To withdraw tokens from the pool itself, use the native `OneResourcePool` method `protected_withdraw`. This requires the super admin badge.

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_super_admin_badge}")
  "create_proof_of_amount"
  Address("{super_admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "get_pool_vault_amount"
;

# Use the amount from the previous call
CALL_METHOD
  Address("{pool_address}")
  "protected_withdraw"
  Decimal("{amount_to_withdraw}")
  Enum<WithdrawStrategy::Rounded>(Enum<RoundingMode::ToZero>())
;

CALL_METHOD
  Address("{your_account_address}")
  "deposit_batch"
  Expression("ENTIRE_WORKTOP")
;
```

## Metadata
The pool units (lp tokens) don't have any metadata (so no name, symbol and icon) on instantiation. We need to use the super admin badge to set this (same for the component and locker, and their metadata). This is fine for testing purposes, in my opinion. So I suggest to not care about that for now.

## Other methods

### Put LP Tokens Back
Returns LP tokens to the component's internal vault (super admin only).

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_super_admin_badge}")
  "create_proof_of_amount"
  Address("{super_admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{your_account_address}")
  "withdraw"
  Address("{lp_token_address}")
  Decimal("{amount}")
;

TAKE_ALL_FROM_WORKTOP
  Address("{lp_token_address}")
  Bucket("lp_tokens")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "put_lp"
  Bucket("lp_tokens")
;
```

### Put Locked Tokens Back
Returns locked tokens to the component's vault (super admin only).

Manifest:
```
CALL_METHOD
  Address("{account_that_holds_super_admin_badge}")
  "create_proof_of_amount"
  Address("{super_admin_badge_address}")
  Decimal("1")
;

CALL_METHOD
  Address("{your_account_address}")
  "withdraw"
  Address("resource_tdx_2_1tknxxxxxxxxxradxrdxxxxxxxxx009923554798xxxxxxxxxtfd2jc") # XRD or your token
  Decimal("{amount}")
;

TAKE_ALL_FROM_WORKTOP
  Address("resource_tdx_2_1tknxxxxxxxxxradxrdxxxxxxxxx009923554798xxxxxxxxxtfd2jc")
  Bucket("tokens")
;

CALL_METHOD
  Address("{incentives_vester_component_address}")
  "put_locked_tokens"
  Bucket("tokens")
;
```

### Query Methods (Public)
These methods can be called by anyone to get information about the vesting state:

- `get_lp_token_amount` - Returns the amount of LP tokens currently in the component's internal vault
- `get_maturity_value` - Returns the projected value of 1 LP token at full maturity (when all tokens are vested)
- `get_pool_vault_amount` - Returns the amount of tokens currently in the pool (available for redemption)
- `get_locked_vault_amount` - Returns the amount of tokens still locked (not yet vested)
- `get_pool_unit_resource_address` - Returns the resource address of the LP tokens
- `get_pool_redemption_value` - Returns the current redemption value for a given amount of LP tokens
- `get_vested_tokens` - Returns the total amount of tokens that have been vested so far
- `get_total_tokens_to_vest` - Returns the total amount of tokens that will be vested over the entire vesting period

Example manifest for query methods:
```
CALL_METHOD
  Address("{incentives_vester_component_address}")
  "get_maturity_value"
;
```
