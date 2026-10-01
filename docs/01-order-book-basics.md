# 1. Order Book Basics

> **Who this is for:** anyone who has never looked inside an exchange. No finance
> background is assumed. If you already know what a "limit order book" is, skim
> the worked example at the end and jump to [chapter 2](02-architecture.md).

## 1.1 What is a matching engine?

A **matching engine** is the part of an exchange that answers one question,
millions of times per day:

> *"Somebody wants to buy, somebody wants to sell. Can we pair them up, and at
> what price?"*

Everything else in this repository (the arena, the bitset, the hash map, ...)
exists to answer that question **fast** and **predictably**.

## 1.2 Orders

An **order** is an instruction from a trader. This engine understands one kind:
the **limit order**. A limit order has four pieces of information:

| Field      | Meaning                                                        | In this code                |
|------------|----------------------------------------------------------------|-----------------------------|
| `side`     | Does the trader want to **buy** or **sell**?                   | `Side::Buy` / `Side::Sell`  |
| `price`    | The *worst* price the trader will accept.                      | `price_tick: u32`           |
| `qty`      | How many units (shares, lots, contracts...).                   | `qty: u64`                  |
| `order_id` | A unique label so the order can be referred to (e.g. cancelled). | `order_id: u64`             |

"Unique" here means *unique among orders that are still in the book*. The engine
rejects a new order whose id belongs to a resting order, but once an order has
been fully filled or cancelled it forgets the id, so the same id can be used
again. A real exchange usually forbids that; here it is the caller's job.

The price is a **limit**, which means:

- A **buy** at 100 says *"I'll pay 100 or less."*
- A **sell** at 100 says *"I'll accept 100 or more."*

## 1.3 The order book

The **order book** is the list of orders that are waiting for a counterparty.
It has two sides:

- **Bids**: buy orders, waiting. The best bid is the **highest** one: it is the
  most generous offer a seller could trade with (the *best bid*).
- **Asks** (a.k.a. offers): sell orders, waiting. The best ask is the **lowest**
  one: it is the cheapest price a buyer could trade at (the *best ask*).

```text
            ASKS  (sellers waiting)
   price    qty
    103      8
    102      5
    101      7        <-- best ask  (lowest sell price)
  ----------------------------------------------  spread
    100      6        <-- best bid  (highest buy price)
     99     10
     98      4
            BIDS  (buyers waiting)
```

The gap between the best bid and the best ask is the **spread**. In a healthy
book the best bid is always **strictly below** the best ask. If they ever touch,
a trade happens immediately. That is exactly the matching engine's job.

> [!NOTE]
> The word **CLOB** in this project's description means *Central Limit Order
> Book*: one central book, made only of limit orders, that everybody trades
> against.

## 1.4 Crossing the spread: when a trade happens

When a new order arrives, the engine first checks whether it can trade right
away. A new order is **marketable** (we also say it *crosses the spread*) when:

- a **buy** has a price **≥ the best ask**, or
- a **sell** has a price **≤ the best bid**.

```text
   best ask = 101

   new BUY @ 100  ->  100 < 101  -> not marketable, goes to rest in the book
   new BUY @ 101  ->  101 >= 101 -> marketable, trades with the ask at 101
   new BUY @ 105  ->  105 >= 101 -> marketable, trades with the ask at 101
```

The last line holds a subtle point that surprises beginners: the buyer said
"up to 105", but the trade happens at **101**, the price of the order that was
already waiting. **Trades always happen at the resting order's price.** The
buyer gets a better deal than they asked for ("price improvement").

### Maker and taker

Two orders take part in every trade:

- The **maker** was already resting in the book. It *made* liquidity.
- The **taker** just arrived and *took* liquidity.

You will see these words everywhere in the code (`maker_order_id`,
`taker_order_id`, `match_incoming`).

## 1.5 Price-time priority

What if several orders are waiting? Who goes first? The rule used by most
exchanges, and by this engine, is **price-time priority**:

1. **Price first.** The best price is served first. For a buyer that means the
   lowest ask; for a seller the highest bid.
2. **Then time.** Among orders at the **same price**, the one that arrived
   **first** is served first (FIFO: first in, first out).

```text
   Price level 101 (asks):   [ #2 sell 3 ]  ->  [ #3 sell 4 ]
                               oldest                newest
                               trades first          trades second
```

This is why every price level in the engine is a **queue**, not just a number
(see [chapter 3](03-order-pool.md)).

## 1.6 Partial fills

An incoming order might be bigger than what is available. Then it is filled
**partially**:

- If the taker is bigger than the maker, the maker is fully filled and
  disappears; the taker continues to the next resting order.
- If the taker is smaller than the maker, the taker is fully filled and the
  maker stays in the book with a smaller remaining quantity.
- If, after trading with everything it can, the taker still has quantity left,
  the **remainder rests** in the book as a new waiting order.

## 1.7 Cancelling

A trader can withdraw a resting order at any time using its `order_id`. The
engine must find the order *without scanning the book*, remove it from the
middle of its queue, and, if it was the last order at that price, remove the
price from the book. Making this O(1) is one of the main design challenges
(see [chapters 3 to 5](03-order-pool.md)).

## 1.8 Ticks: prices as small integers

Real prices look like `101.25`. Floating-point numbers cannot represent most
decimal fractions exactly and give surprising results (`0.1 + 0.2 != 0.3`), which is
unacceptable when money changes hands, so exchanges use
**integer ticks**. A *tick* is the smallest allowed price step.

```text
   tick size = 0.25          tick 0   -> price 100.00   (if we choose an offset of 100.00)
                             tick 1   -> price 100.25
                             tick 2   -> price 100.50
                             ...
                             tick 4095-> price 1123.75
```

This engine works **only in ticks**: `price_tick` is a `u32` between `0` and
`4095` inclusive (4096 distinct prices). Converting between real prices and
ticks (`price = offset + tick * tick_size`) is the **caller's job**.

Why this matters for the design: if the set of possible prices is small and
dense, we can use **arrays indexed by tick** instead of trees or hash maps.
`levels[tick]` is a single memory lookup. That idea powers the whole design.

## 1.9 What this engine supports (and does not)

| Supported                                              | Not supported (yet)                                    |
|--------------------------------------------------------|--------------------------------------------------------|
| Limit orders, buy and sell                             | Market orders, stop orders                             |
| Partial and full fills                                 | Time-in-force variants (IOC, FOK, day orders)          |
| Resting the unfilled remainder                         | Modify / replace an order (cancel + new works)         |
| Cancel by `order_id`                                   | Self-trade prevention, per-account limits              |
| Price-time priority                                    | More than one instrument per `Engine` (use one each)   |
| Depth queries (`depth_at`, `depth_snapshot`)           | Auctions, circuit breakers, fees                       |

Keeping the feature set small is deliberate: it keeps the code short enough to
read in one sitting. [Chapter 10](10-limitations-and-exercises.md) lists
extension ideas.

## 1.10 A complete worked example

Follow this example carefully; we reuse it in
[chapter 6](06-matching-algorithm.md) to show what the engine does internally.
Every output below was produced by running the real engine.

**Step 1 to 5: five orders arrive, none can trade, so all of them rest.**

| # | Arrival                | Marketable? | Result       |
|---|------------------------|-------------|--------------|
| 1 | `#1` SELL 5 @ 102      | no bids yet | rests        |
| 2 | `#2` SELL 3 @ 101      | no bids yet | rests        |
| 3 | `#3` SELL 4 @ 101      | no bids yet | rests, **behind** `#2` |
| 4 | `#4` BUY 10 @ 99       | 99 < best ask 101 | rests  |
| 5 | `#5` BUY 6 @ 100       | 100 < best ask 101 | rests |

The book now looks like this:

```text
            ASKS
   tick   total   queue (oldest -> newest)
    102      5    [#1:5]
    101      7    [#2:3] -> [#3:4]        <-- best ask = 101
  ---------------------------------------------------------
    100      6    [#5:6]                  <-- best bid = 100
     99     10    [#4:10]
            BIDS
```

**Step 6: `#6` BUY 14 @ 102 arrives.** It is marketable (102 ≥ 101). The engine
walks up the ask side, cheapest first:

| Fill | Against      | Price | Qty | Why                                                  |
|------|--------------|-------|-----|------------------------------------------------------|
| 1    | `#2` (maker) | 101   | 3   | oldest at the best price; `#2` is fully filled       |
| 2    | `#3` (maker) | 101   | 4   | next in the FIFO queue; `#3` is fully filled         |
| 3    | `#1` (maker) | 102   | 5   | level 101 is now empty, move to 102; `#1` fully filled |

Total filled: 3 + 4 + 5 = 12. The order asked for 14, so **2 units remain**.
There are no more asks, so the remainder **rests as a bid at 102**:

```text
            ASKS
   (empty)
  ---------------------------------------------------------
    102      2    [#6:2]                  <-- best bid = 102   (the remainder)
    100      6    [#5:6]
     99     10    [#4:10]
            BIDS
```

Then `#4` is cancelled: the bid at 99 disappears, leaving bids at 102 and 100.

The exact sequence of **events** the engine emitted for step 6 was:

```text
OrderAccepted      { order_id: 6 }
Trade              { maker_order_id: 2, taker_order_id: 6, price_tick: 101, qty: 3 }
OrderFilled        { order_id: 2 }
Trade              { maker_order_id: 3, taker_order_id: 6, price_tick: 101, qty: 4 }
OrderFilled        { order_id: 3 }
Trade              { maker_order_id: 1, taker_order_id: 6, price_tick: 102, qty: 5 }
OrderFilled        { order_id: 1 }
OrderPartiallyFilled { order_id: 6, remaining_qty: 2 }
```

Events are how the engine talks to the outside world; see
[chapter 7](07-events.md).

## 1.11 Glossary

| Term            | Meaning                                                                  |
|-----------------|--------------------------------------------------------------------------|
| Bid             | A resting buy order                                                      |
| Ask / Offer     | A resting sell order                                                     |
| Best bid / ask  | Highest bid / lowest ask, the most attractive prices on each side        |
| Spread          | Best ask minus best bid                                                  |
| Limit order     | "Trade at this price or better"                                          |
| Marketable      | Able to trade immediately against the opposite side                      |
| Maker           | The resting order in a trade                                             |
| Taker           | The incoming order in a trade                                            |
| Price level     | All orders resting at one price                                          |
| Depth           | Total quantity available at a price level (or across several)            |
| Fill            | An execution; *partial fill* means some quantity is left                 |
| Tick            | Smallest price step; this engine uses integer ticks 0 to 4095            |
| Price-time priority | Better price first, then earlier arrival                             |
| Arena           | A big pre-allocated array that objects live in (chapter 3)               |
| Hot path        | The code that runs for every order; the part that must be fastest        |

**Next:** [2. Architecture overview](02-architecture.md)
