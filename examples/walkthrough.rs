//! The worked example from chapter 1 (section 1.10), step by step.
//!
//! Every order is sent to a fresh engine, and after each step the program
//! prints the events the engine emitted and the book as a ladder of price
//! levels. Compare the output with the tables and diagrams in chapter 1.
//!
//! Run it with `cargo run --example walkthrough`.

use matching_engine::{Engine, Event, Side};

/// Prints both sides of the book: asks on top (highest price first), then
/// bids (highest price first), so the best prices meet in the middle.
fn print_book(engine: &Engine) {
    let asks = engine.depth_snapshot(Side::Sell, usize::MAX); // lowest first
    let bids = engine.depth_snapshot(Side::Buy, usize::MAX); // highest first

    println!("        ASKS");
    println!("   tick   total");
    if asks.is_empty() {
        println!("   (empty)");
    }
    for (tick, qty) in asks.iter().rev() {
        let marker = if Some(*tick) == engine.best_ask() {
            "  <-- best ask"
        } else {
            ""
        };
        println!("   {tick:>4}   {qty:>5}{marker}");
    }
    println!("  -------------------------");
    for (tick, qty) in &bids {
        let marker = if Some(*tick) == engine.best_bid() {
            "  <-- best bid"
        } else {
            ""
        };
        println!("   {tick:>4}   {qty:>5}{marker}");
    }
    if bids.is_empty() {
        println!("   (empty)");
    }
    println!("        BIDS");
}

/// Submits one limit order, then shows what happened.
fn place(engine: &mut Engine, order_id: u64, side: Side, price_tick: u32, qty: u64) {
    println!("\n=== #{order_id} {side:?} {qty} @ {price_tick} ===");
    let mut events: Vec<Event> = Vec::new();
    engine.process_order(order_id, side, price_tick, qty, &mut events);
    for event in &events {
        println!("  {event:?}");
    }
    print_book(engine);
}

fn main() {
    let mut engine = Engine::new(16);

    // Steps 1 to 5: nothing can trade yet, so every order rests.
    place(&mut engine, 1, Side::Sell, 102, 5);
    place(&mut engine, 2, Side::Sell, 101, 3);
    place(&mut engine, 3, Side::Sell, 101, 4); // queues behind #2 at 101
    place(&mut engine, 4, Side::Buy, 99, 10);
    place(&mut engine, 5, Side::Buy, 100, 6);
    assert_eq!(engine.best_ask(), Some(101));
    assert_eq!(engine.best_bid(), Some(100));

    // Step 6: a buy that crosses the spread. It fills against #2, #3 and #1
    // (best price first, oldest first), then 2 units rest as a bid at 102.
    place(&mut engine, 6, Side::Buy, 102, 14);
    assert_eq!(engine.best_ask(), None);
    assert_eq!(engine.best_bid(), Some(102));
    assert_eq!(engine.depth_at(Side::Buy, 102), 2);

    // Finally, cancel #4: the bid at 99 disappears.
    println!("\n=== cancel #4 ===");
    let mut events: Vec<Event> = Vec::new();
    let found = engine.cancel_order(4, &mut events);
    println!("  found: {found}");
    for event in &events {
        println!("  {event:?}");
    }
    print_book(&engine);
    assert_eq!(engine.depth_at(Side::Buy, 99), 0);
    assert_eq!(engine.live_order_count(), 2); // #6 and #5
}
