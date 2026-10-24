//! `lezer_token` — fungible token program for LEZ v0.3.
//!
//! Accounts per instruction are documented on `token_core::Instruction`. Authorization is
//! expressed by which input accounts the caller signed (`is_authorized`); the guest derives
//! every PDA it writes, so a caller cannot point effects at accounts it does not control.

use borsh::BorshDeserialize as _;
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    program::{ApplyInput, ChainedCall, PdaSeed, Plan, ProgramCall, apply_write, read_program_call},
};
use token_core::{Effect, Holding, Instruction, TokenDefinition, apply_supply_delta, holding_seed, validate_definition};

fn main() {
    match read_program_call::<Instruction>() {
        ProgramCall::Plan(input, instruction) => plan(&input, instruction).write(),
        ProgramCall::Apply(input) => apply(input),
    }
}

fn plan(input: &lee_core::program::PlanInput, instruction: Instruction) -> Plan {
    let mut plan = Plan::new(input);
    match instruction {
        Instruction::Define { name, symbol, decimals, authority, initial_supply } => {
            validate_definition(&name, &symbol, decimals).unwrap_or_else(|e| panic!("{e}"));
            let [definition, creator_holding] = input.accounts.as_slice() else {
                panic!("Define expects [definition, creator_holding]");
            };
            assert!(definition.is_authorized, "the definition account must sign");
            let def_bytes = *definition.account_id.value();
            let creator = authority.unwrap_or(def_bytes);
            let expected = pda(input, holding_seed(&def_bytes, &creator));
            assert_eq!(creator_holding.account_id, expected, "creator holding must be the PDA of (definition, authority-or-creator)");
            let def = TokenDefinition { version: 1, name, symbol, decimals, authority, total_supply: initial_supply };
            plan.effect(definition, &Effect::Define { def, initial_supply });
            plan.effect(creator_holding, &Effect::Credit { definition: def_bytes, amount: initial_supply });
        }
        Instruction::Mint { owner, amount } => {
            let [authority, definition, to_holding] = input.accounts.as_slice() else {
                panic!("Mint expects [authority, definition, recipient_holding]");
            };
            assert!(authority.is_authorized, "the mint authority must sign");
            let def_bytes = *definition.account_id.value();
            let expected = pda(input, holding_seed(&def_bytes, &owner));
            assert_eq!(to_holding.account_id, expected, "recipient holding is not derived from the owner");
            plan.effect(definition, &Effect::UpdateSupply { authority: *authority.account_id.value(), delta_minted: amount_to_i128(amount), delta_burned: 0 });
            plan.effect(to_holding, &Effect::Credit { definition: def_bytes, amount });
        }
        Instruction::Transfer { to_owner, amount } => {
            let [owner, definition, from_holding, to_holding] = input.accounts.as_slice() else {
                panic!("Transfer expects [owner, definition, from_holding, to_holding]");
            };
            assert!(owner.is_authorized, "the sender must sign");
            let owner_bytes = *owner.account_id.value();
            let def_bytes = *definition.account_id.value();
            assert_eq!(from_holding.account_id, pda(input, holding_seed(&def_bytes, &owner_bytes)), "from_holding is not the sender's holding");
            assert_eq!(to_holding.account_id, pda(input, holding_seed(&def_bytes, &to_owner)), "to_holding is not the recipient's holding");
            plan.effect(from_holding, &Effect::Spend { definition: def_bytes, amount });
            plan.effect(to_holding, &Effect::Credit { definition: def_bytes, amount });
        }
        Instruction::Burn { amount } => {
            let [owner, definition, holding] = input.accounts.as_slice() else {
                panic!("Burn expects [owner, definition, holding]");
            };
            assert!(owner.is_authorized, "the holder must sign");
            let def_bytes = *definition.account_id.value();
            assert_eq!(holding.account_id, pda(input, holding_seed(&def_bytes, owner.account_id.value())), "holding is not the owner's holding");
            plan.effect(definition, &Effect::UpdateSupply { authority: *owner.account_id.value(), delta_minted: 0, delta_burned: amount_to_i128(amount) });
            plan.effect(holding, &Effect::Spend { definition: def_bytes, amount });
        }
        Instruction::SetAuthority { new_authority } => {
            let [authority, definition] = input.accounts.as_slice() else {
                panic!("SetAuthority expects [current_authority, definition]");
            };
            assert!(authority.is_authorized, "the current authority must sign");
            plan.effect(definition, &Effect::SetAuthority { current: *authority.account_id.value(), new: new_authority });
        }
        Instruction::RegisterSelf(params) => {
            return register_self(input, params);
        }
    }
    plan
}

/// Emits a chained call to the registry keyed by this program's account id — see the
/// `lezer_registry` docs; registration can't be forged by any signer.
fn register_self(input: &lee_core::program::PlanInput, params: registry_core::RegisterSelfParams) -> Plan {
    let registry = AccountId::new(params.registry);
    let [entry, index] = input.accounts.as_slice() else {
        panic!("RegisterSelf expects [entry, index_page] registry accounts");
    };
    let self_bytes = *input.self_account_id.value();
    assert_eq!(
        entry.account_id,
        AccountId::for_public_pda(&registry, &PdaSeed::new(registry_core::entry_seed(&self_bytes))),
        "entry is not the registry's PDA for this program"
    );
    assert_eq!(
        index.account_id,
        AccountId::for_public_pda(
            &registry,
            &PdaSeed::new(registry_core::index_page_seed(registry_core::bucket_of(&self_bytes), params.page)),
        ),
        "index_page is not the expected registry page"
    );
    let mut plan = Plan::new(input);
    plan.call(ChainedCall::new(
        registry,
        vec![
            ProgramShardSelector::new(entry.account_id, registry),
            ProgramShardSelector::new(index.account_id, registry),
        ],
        &registry_core::Instruction::Register {
            name: params.name,
            source_url: params.source_url,
            commit: params.commit,
            version: params.version,
        },
    ));
    plan
}

fn pda(input: &lee_core::program::PlanInput, seed: [u8; 32]) -> AccountId {
    AccountId::for_public_pda(&input.self_account_id, &PdaSeed::new(seed))
}

fn amount_to_i128(amount: u128) -> i128 {
    i128::try_from(amount).expect("amount exceeds i128")
}

fn apply(input: ApplyInput) -> ! {
    let effect = Effect::try_from_slice(&input.effect_data).expect("a program only applies effects it planned");
    let post: Vec<u8> = match effect {
        Effect::Define { def, initial_supply } => {
            assert!(input.pre_data.is_empty(), "token already defined on this account");
            assert_eq!(def.total_supply, initial_supply);
            borsh::to_vec(&def).expect("borsh serialization is infallible")
        }
        Effect::UpdateSupply { authority, delta_minted, delta_burned } => {
            let mut def = TokenDefinition::try_from_slice(&input.pre_data).expect("not a token definition");
            if delta_minted > 0 {
                assert_eq!(def.authority, Some(authority), "caller is not the mint authority");
            }
            def.total_supply = apply_supply_delta(def.total_supply, delta_minted, delta_burned)
                .unwrap_or_else(|e| panic!("{e}"));
            borsh::to_vec(&def).expect("borsh serialization is infallible")
        }
        Effect::Credit { definition, amount } => {
            let mut h = if input.pre_data.is_empty() {
                Holding { definition, balance: 0 }
            } else {
                Holding::try_from_slice(&input.pre_data).expect("corrupt holding")
            };
            assert_eq!(h.definition, definition, "holding belongs to a different token");
            h.balance = h.balance.checked_add(amount).expect("balance overflow");
            borsh::to_vec(&h).expect("borsh serialization is infallible")
        }
        Effect::Spend { definition, amount } => {
            let mut h = Holding::try_from_slice(&input.pre_data).expect("holding does not exist");
            assert_eq!(h.definition, definition, "holding belongs to a different token");
            h.balance = h.balance.checked_sub(amount).expect("insufficient token balance");
            borsh::to_vec(&h).expect("borsh serialization is infallible")
        }
        Effect::SetAuthority { current, new } => {
            let mut def = TokenDefinition::try_from_slice(&input.pre_data).expect("not a token definition");
            assert_eq!(def.authority, Some(current), "caller is not the authority");
            def.authority = new;
            borsh::to_vec(&def).expect("borsh serialization is infallible")
        }
    };
    apply_write(input, post.try_into().expect("shard data fits the limit"))
}
