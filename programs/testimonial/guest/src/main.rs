//! `lezer_testimonial` — LP-0021 on-chain testimonial program for LEZ v0.3.
//!
//! Accounts: `[author, index_page]`, both addressed at this program's shard.
//! Plan enforces: author signed; index page is the PDA for the author's bucket and the
//! requested page. Apply enforces: one testimonial per author; page capacity; no duplicates.

use borsh::BorshDeserialize as _;
use lee_core::{
    account::AccountId,
    program::{ApplyInput, PdaSeed, Plan, ProgramCall, apply_write, read_program_call},
};
use lee_core::{account::ProgramShardSelector, program::ChainedCall};
use testimonial_core::{Effect, Instruction, Submit, Testimonial, append_author, bucket_of, page_seed, validate};

fn main() {
    match read_program_call::<Instruction>() {
        ProgramCall::Plan(input, Instruction::Submit(submit)) => {
            if let Err(e) = validate(&submit) {
                panic!("{e}");
            }
            let [author, index] = input.accounts.as_slice() else {
                panic!("expected accounts [author, index_page]");
            };
            assert!(author.is_authorized, "author must sign the testimonial");

            let author_bytes: [u8; 32] = *author.account_id.value();
            let expected_index = AccountId::for_public_pda(
                &input.self_account_id,
                &PdaSeed::new(page_seed(bucket_of(&author_bytes), submit.page)),
            );
            assert_eq!(index.account_id, expected_index, "wrong index page for this author");

            let mut plan = Plan::new(&input);
            plan.effect(
                author,
                &Effect::WriteRecord(Testimonial { version: 1, username: submit.username, text: submit.text }),
            );
            plan.effect(index, &Effect::AppendAuthor(author_bytes));
            plan.write()
        }
        ProgramCall::Plan(input, Instruction::RegisterSelf(params)) => register_self(&input, params).write(),
        ProgramCall::Apply(input) => apply(input),
    }
}

/// Emits a chained call to the registry; the registry sees `caller_account_id = this program`,
/// so the listing is keyed by the program itself — the accounts only carry the PDAs.
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
        Effect::WriteRecord(record) => {
            assert!(input.pre_data.is_empty(), "this account already submitted a testimonial");
            borsh::to_vec(&record).expect("borsh serialization is infallible")
        }
        Effect::AppendAuthor(author) => {
            let page = append_author(&input.pre_data, author).unwrap_or_else(|e| panic!("{e}"));
            borsh::to_vec(&page).expect("borsh serialization is infallible")
        }
    };
    let data = post.try_into().expect("record fits within the shard data limit");
    apply_write(input, data)
}
