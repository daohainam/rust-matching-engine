# 1. Order Book Basics

> **New word?** Every technical term in these chapters is explained in the
> [glossary](glossary.md).

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

An **order** is an instruction from a trader. The basic kind is the **limit
order**; the other kinds this engine understands are variations on it (1.9). A
limit order has four pieces of information:

| Field      | Meaning                                                        | In this code                |
|------------|----------------------------------------------------------------|-----------------------------|
| `side`     | Does the trader want to **buy** or **sell**?                   | `Side::Buy` / `Side::Sell`  |
| `price`    | The *worst* price the trader will accept.                      | `price_tick: u32`           |
| `qty`      | How many units (shares, lots, contracts...).                   | `qty: u64`                  |
| `order_id` | A unique label so the order can be referred to (e.g. cancelled). | `order_id: u64`             |
| `account`  | Who placed it: a trader, a firm, or whatever unit the caller tracks. | `account: u32`              |

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
- The **taker** just arrived and *took* liquidity. It is also called the
  **aggressor**, and its side is the trade's **aggressor side**: a trade where
  a buyer crossed the spread was "buyer-initiated".

You will see these words everywhere in the code (`maker_order_id`,
`taker_order_id`, `taker_side`, `match_incoming`).

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
- If both have exactly the same quantity, **both** are fully filled at once: the
  maker leaves the book and the taker is done. (The engine reports an
  `OrderFilled` event for each of them.)
- If, after trading with everything it can, the taker still has quantity left,
  the **remainder rests** in the book as a new waiting order.

## 1.7 Cancelling

A trader can withdraw a resting order at any time using its `order_id`. The
engine must find the order *without scanning the book*, remove it from the
middle of its queue, and, if it was the last order at that price, remove the
price from the book. Making this O(1) is one of the main design challenges
(see [chapters 3 to 5](03-order-pool.md)).

A trader can also **reduce** a resting order's quantity. The order keeps its
place in the queue: a smaller order never gets ahead of anyone, so letting it
stay is fair. Raising the quantity, or changing the price, would be unfair to
the orders queued behind it, so for that the trader cancels and sends a new
order, which joins the back of the queue.

Sometimes every order of one trader must go at once: their connection drops
(many exchanges then "cancel on disconnect"), or a risk system pulls the plug.
A **mass cancel** does that for one account, or for the whole book at the end
of a session or when trading halts.

### Trading with yourself

A large trader often has buy and sell orders in the book at the same time, from
different strategies. If one of their buys meets one of their own sells, a
"trade" is printed that moved nothing from one owner to another, but that the
rest of the market sees as real activity. Done on purpose that is called **wash
trading**, and it is illegal market manipulation. So venues offer **self-trade
prevention**: when an incoming order would trade with a resting order of the
same account, the trade is blocked, and the trader chooses what happens instead:
cancel the resting order, cancel the incoming one, or both.

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

This engine works **only in ticks**: `price_tick` is a `u32` from `0` up to the
engine's tick count, which is 4096 unless you choose another one with
`Engine::with_ticks` (up to about 16.7 million). The example above needs
4096 ticks to reach 1123.75; a share around 100.00 traded in steps of 0.01 needs
more than 10,000. Converting between real prices and ticks
(`price = offset + tick * tick_size`) is the **caller's job**.

The chapters use the default 4096 ticks in their examples.

Why this matters for the design: if the set of possible prices is small and
dense, we can use **arrays indexed by tick** instead of trees or hash maps.
`levels[tick]` is a single memory lookup. That idea powers the whole design.

## 1.9 What this engine supports (and does not)

| Supported                                              | Not supported (yet)                                    |
|--------------------------------------------------------|--------------------------------------------------------|
| Limit, immediate-or-cancel, market and post-only orders | Stop orders, fill-or-kill, day orders                 |
| Partial and full fills                                 | Iceberg orders (hidden quantity)                       |
| Resting the unfilled remainder                         | Raising quantity or changing price in place (cancel + new works) |
| Cancel by `order_id`; reduce quantity, keeping priority | Per-account limits (position, open orders)         |
| Accounts, self-trade prevention, mass cancel           | Trading sessions and halts (only `cancel_all` for them) |
| Price-time priority                                    | More than one instrument per `Engine` (use one each)   |
| Depth queries and order lookup                         | Auctions, circuit breakers, fees                       |
| A tick count chosen per engine, up to 2^24             | Prices that are not whole ticks                        |
| Pre-trade risk limits: maximum quantity, price band    | Per-account risk (positions, credit)                   |

### Risk limits: stopping the fat finger

A trader who types 10000 instead of 100, or a buy price ten times too high, can
do real damage in microseconds: the order trades against everything it reaches.
Exchanges therefore check every order against **pre-trade risk limits** before it
may trade. This engine has two:

- a **maximum order quantity**;
- a **price band**: an order may reach at most a few ticks past the best price
  on the other side. A buy limited at 150 when the best ask is 100 is refused
  outright, and a market order stops at the edge of the band instead of
  sweeping the whole book.

The limits are part of the engine's state and can change during the day, for
example when a market gets volatile.

### Order types

Every type trades the same way, best price first and oldest order first. They
differ in what may trade at all, and in what happens to the quantity that could
not trade at once:

| Type | Trades on arrival | Quantity left over | In the code |
|---|---|---|---|
| **Limit** | at its price or better | rests in the book | `OrderType::Limit` |
| **Immediate-or-cancel** (IOC) | at its price or better | expires, never rests | `OrderType::ImmediateOrCancel` |
| **Market** | at any price | expires, never rests | `OrderType::Market` |
| **Post-only** | never: refused if it would | rests in the book | `OrderType::PostOnly` |

A **market** order says "buy 10 now, whatever it costs". It has no price of its
own, so it may sweep through many levels; that is why real exchanges protect it
with price bands, as this engine does when one is set (see *Risk limits* above). A
**post-only** order
is for market makers, who want to be sure they only ever *add* orders to the
book (and often pay lower fees for it): if it would trade on arrival, it is
refused instead.

### What happens to a bad order?

Some orders are refused before they can touch the book. The engine reports an
`OrderRejected` event that says why, and nothing else changes:

| The order has... | Reason reported |
|---|---|
| a quantity of 0 | `ZeroQuantity` |
| the id `u64::MAX` (reserved for internal use, see [chapter 5](05-hash-map.md)) | `ReservedOrderId` |
| a price tick outside the engine's range (4096 or more by default) | `PriceOutOfRange` |
| the same id as an order that is still resting | `DuplicateOrderId` |
| a quantity above the engine's maximum order quantity | `QuantityAboveLimit` |
| a price further past the best opposing price than the price band allows | `PriceOutsideBand` |
| post-only, and a price that would trade at once | `PostOnlyWouldTrade` |

An order can also be accepted and trade normally, but then find **no room** to
rest its remainder, because the book is full. It is rejected at that point with
`BookFull` (chapter 6 explains when this can happen).

Keeping the feature set small is deliberate: it keeps the code short enough to
read in one sitting. [Chapter 11](11-limitations-and-exercises.md) lists
extension ideas.

## 1.10 A complete worked example

Follow this example carefully; we reuse it in
[chapter 6](06-matching-algorithm.md) to show what the engine does internally.
Every output below was produced by running the real engine, and you can
reproduce it yourself:

```sh
cargo run --example walkthrough
```

The program ([`examples/walkthrough.rs`](../examples/walkthrough.rs)) sends the
orders below one by one and prints the events and the book after each step.
Its book shows the total quantity per price; the queues of individual orders in
the diagrams below are what chapters 3 and 6 build.

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
OrderAccepted { order_id: 6, side: Buy, price_tick: 102, qty: 14 }
Trade { trade_id: 1, maker_order_id: 2, taker_order_id: 6, taker_side: Buy, price_tick: 101, qty: 3 }
OrderFilled { order_id: 2 }
Trade { trade_id: 2, maker_order_id: 3, taker_order_id: 6, taker_side: Buy, price_tick: 101, qty: 4 }
OrderFilled { order_id: 3 }
Trade { trade_id: 3, maker_order_id: 1, taker_order_id: 6, taker_side: Buy, price_tick: 102, qty: 5 }
OrderFilled { order_id: 1 }
OrderPartiallyFilled { order_id: 6, remaining_qty: 2 }
```

Every `Trade` says who was the aggressor (`taker_side: Buy`: the buyer crossed
the spread) and carries a `trade_id`. These are the first three trades of this
book, so they are numbered 1, 2 and 3.

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
| Market order    | "Trade now, at any price"; what cannot trade at once expires              |
| IOC             | Immediate-or-cancel: a limit order whose remainder expires instead of resting |
| Post-only       | A limit order that is refused if it would trade on arrival               |
| Account         | Who placed an order; self-trade prevention and mass cancels use it       |
| Self-trade      | A trade between two orders of the same account; can be prevented         |
| Price band      | How far past the best opposing price an order may reach                  |
| Marketable      | Able to trade immediately against the opposite side                      |
| Maker           | The resting order in a trade                                             |
| Taker           | The incoming order in a trade                                            |
| Price level     | All orders resting at one price                                          |
| Depth           | Total quantity available at a price level (or across several)            |
| Fill            | An execution; *partial fill* means some quantity is left                 |
| Tick            | Smallest price step; this engine uses integer ticks, 0 to 4095 by default |
| Price-time priority | Better price first, then earlier arrival                             |
| Arena           | A big pre-allocated array that objects live in (chapter 3)               |
| Hot path        | The code that runs for every order; the part that must be fastest        |

This table covers the trading words used in this chapter. The full
[glossary](glossary.md) also explains every data-structure, Rust, concurrency
and storage term the later chapters use.

## 1.12 Try it yourself

Run the worked example and compare its output with 1.10:

```sh
cargo run --example walkthrough
```

Then change it. In [`examples/walkthrough.rs`](../examples/walkthrough.rs), make
`#6` a `Buy 14 @ 101` instead of `@ 102`. **Before running it**, predict which
makers it trades with, how much is left over, and at what price the remainder
rests. (The `assert_eq!` lines after step 6 will fail with the new numbers;
update them to your prediction.)

The unit test that pins down price-time priority is a short, readable example
of the engine's API:

```sh
cargo test price_time_priority_within_a_level_is_fifo
```

**Next:** [2. Architecture overview](02-architecture.md)
