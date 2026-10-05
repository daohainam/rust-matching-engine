//! Runs a random request stream through one engine, restores a second
//! engine from a snapshot taken half-way, and requires both to answer the
//! rest of the stream with identical events and to end with identical
//! books.

mod common;

use common::{Op, OpGenerator};
use matching_engine::{Engine, Event};

fn apply(engine: &mut Engine, op: Op, events: &mut Vec<Event>) {
    match op {
        Op::Place(order) => engine.submit_order(order, events),
        Op::Cancel { order_id } => {
            engine.cancel_order(order_id, events);
        }
        Op::Reduce {
            order_id,
            reduce_by,
        } => {
            engine.reduce_order(order_id, reduce_by, events);
        }
        Op::CancelAccount { account } => {
            engine.cancel_account(account, events);
        }
        Op::CancelAll => {
            engine.cancel_all(events);
        }
    }
}

#[test]
fn a_restored_engine_continues_exactly_like_the_original() {
    for (seed, capacity) in [(11, 64), (12, 4_096)] {
        let mut original = Engine::new(capacity);
        let mut generator = OpGenerator::new(seed);
        let mut events = Vec::new();
        for _ in 0..10_000 {
            apply(&mut original, generator.next_op(), &mut events);
        }

        let snapshot = original.snapshot();
        assert_eq!(snapshot.orders.len(), original.live_order_count());
        let mut restored = Engine::restore(&snapshot).expect("an engine's own snapshot restores");
        assert_eq!(restored.snapshot(), snapshot, "seed {seed}");

        for step in 0..10_000 {
            let op = generator.next_op();
            let mut expected = Vec::new();
            let mut actual = Vec::new();
            apply(&mut original, op, &mut expected);
            apply(&mut restored, op, &mut actual);
            assert_eq!(actual, expected, "seed {seed}, step {step}: {op:?}");
        }
        assert_eq!(restored.snapshot(), original.snapshot(), "seed {seed}");
    }
}
