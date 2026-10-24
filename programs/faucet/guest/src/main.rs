//! `lezer_faucet` — LP-0021 on-chain native LEZ faucet program for LEZ v0.3.
//!
//! Dispenses `DEFAULT_DRIP_AMOUNT` (50 LEZ) from the program's treasury PDA (`VAULT_SEED`)
//! to an authorized recipient via `lee_core::native_token::custody_transfer`, while recording
//! a per-recipient `ClaimRecord` PDA to enforce rate-limiting on chain.

use borsh::BorshDeserialize as _;
use faucet_core::{ClaimRecord, DEFAULT_DRIP_AMOUNT, Effect, Instruction, VAULT_SEED, claim_seed};
use lee_core::{
    account::{AccountId, ProgramShardSelector},
    native_token::custody_transfer,
    program::{ApplyInput, ChainedCall, PdaSeed, Plan, ProgramCall, apply_write, read_program_call},
};

fn main() {
    match read_program_call::<Instruction>() {
        ProgramCall::Plan(input, instruction) => plan(&input, instruction).write(),
        ProgramCall::Apply(input) => apply(input),
    }
}

fn plan(input: &lee_core::program::PlanInput, instruction: Instruction) -> Plan {
    match instruction {
        Instruction::Claim => {
            let [vault, recipient, claim_ticket] = input.accounts.as_slice() else {
                panic!("Claim expects [vault_pda, recipient, claim_ticket_pda]");
            };
            assert!(recipient.is_authorized, "the recipient account must authorize the faucet claim");
            let expected_vault = AccountId::for_public_pda(&input.self_account_id, &PdaSeed::new(VAULT_SEED));
            assert_eq!(vault.account_id, expected_vault, "vault is not the faucet treasury PDA");
            let recipient_bytes = *recipient.account_id.value();
            let expected_ticket = AccountId::for_public_pda(
                &input.self_account_id,
                &PdaSeed::new(claim_seed(&recipient_bytes)),
            );
            assert_eq!(claim_ticket.account_id, expected_ticket, "claim_ticket is not derived from the recipient");

            let mut plan = Plan::new(input);
            plan.effect(
                claim_ticket,
                &Effect::RecordClaim {
                    recipient: recipient_bytes,
                    amount: DEFAULT_DRIP_AMOUNT,
                },
            );
            plan.call(custody_transfer(
                vault.account_id,
                PdaSeed::new(VAULT_SEED),
                recipient.account_id,
                DEFAULT_DRIP_AMOUNT,
            ));
            plan
        }
        Instruction::RegisterSelf(params) => register_self(input, params),
    }
}

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

fn apply(input: ApplyInput) -> ! {
    let effect = Effect::try_from_slice(&input.effect_data).expect("a program only applies effects it planned");
    let post = match effect {
        Effect::RecordClaim { recipient, amount } => {
            assert!(
                input.pre_data.is_empty(),
                "faucet rate limit exceeded: account already claimed its allocation"
            );
            let record = ClaimRecord {
                version: 1,
                recipient,
                claims: 1,
                total_claimed: amount,
            };
            borsh::to_vec(&record).expect("borsh serialization is infallible")
        }
    };
    let data = post.try_into().expect("claim record fits within the shard data limit");
    apply_write(input, data)
}
