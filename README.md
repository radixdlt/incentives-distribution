# Incentives Vester Blueprint
This blueprint has been deployed on Stokenet for testing at package address: `package_tdx_2_1p4hhfatdepwarwmmnafzq7dv6mv24kz8qleutkkrnx9tdsmnuudhav`

## Instantiating a vesting component
In the current version, the component must be seeded with all XRD at instantiation. We might want to change this, to be possible after instantiation (possibly in trenches). A *pre-claim period* might need to be built still too.

At instantiation, a **fungible** admin badge must be passed (create at https://stokenet-console.radixdlt.com/create-token), along with a `vest_start` and `vest_end` unix timestamp, and an `initial_vest` fraction (for instance, `0.2` would mean 20% of the total XRD is immediately in the pool, claimable for LP token owners).

Instantiation manifest:
```
CALL_METHOD
  Address("{your_account_address_that_holds_the_xrd_rewards}")
  "withdraw"
  Address("resource_tdx_2_1tknxxxxxxxxxradxrdxxxxxxxxx009923554798xxxxxxxxxtfd2jc") # This is XRD
  Decimal("10000")
;

TAKE_ALL_FROM_WORKTOP
  Address("resource_tdx_2_1tknxxxxxxxxxradxrdxxxxxxxxx009923554798xxxxxxxxxtfd2jc") # This is XRD
  Bucket("rewards")
;

CALL_METHOD
  Address("{your_account_address_that_holds_the_admin_badge}")
  "withdraw"
  Address("{admin_badge_address}") # Create this yourself
  Decimal("1")
;

TAKE_ALL_FROM_WORKTOP
  Address("{admin_badge_address}")
  Bucket("admin_badge")
;

CALL_FUNCTION
  Address("package_tdx_2_1p4hhfatdepwarwmmnafzq7dv6mv24kz8qleutkkrnx9tdsmnuudhav")
  "IncentivesVester"
  "instantiate"
  Bucket("admin_badge")
  1764241121i64 # unix timestamp, vest start
  1766833121i64 # unix timestamp, vest end
  Decimal("0.2") # initial vest (20% immediately claimable at vest start here)
  Bucket("rewards")
  Address("{dapp_definition_address}") # for testing, feel free to use a random account address, it doesn't matter
;

CALL_METHOD
  Address("{your_account_address}") # this will get back the admin badge
  "deposit_batch"
  Expression("ENTIRE_WORKTOP")
;
```

## Claiming LP
To claim LP for a user, we need to sign the transaction for them. We should use this manifest:
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
  Address("{account_to_give_lp_tokens}")
;
```

## Redeem
If the user wants to claim their already vested tokens (and forfeit the other portion), they can use this manifest:
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
The tokens aren't vested automatically, so to show the correct amount of vested tokens in the pool unit, in the wallet, we need to call the refill method. This method is automatically called in the above redeem manifest, so it's not needed to add there. This is purely to have an accurate representation of the worth of the LP tokens in the wallet. We might want to call it once an hour (or less).

Manifest:
```
CALL_METHOD
  Address("{incentives_vester_component_address}")
  "refill"
;
```

## Metadata
The pool units (lp tokens) don't have any metadata (so no name, symbol and icon) on instantiation. We need to use the admin badge to set this (same for the component and locker, and their metadata). This is fine for testing purposes, in my opinion. So I suggest to not care about that for now.
