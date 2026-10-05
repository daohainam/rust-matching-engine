# 6. Thuật toán khớp lệnh

> *Bản gốc tiếng Anh: [06-matching-algorithm.md](../06-matching-algorithm.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Mã nguồn: [`src/engine.rs`](../../src/engine.rs)

Đây là chương mà mọi thứ ghép lại với nhau. Nếu chưa đọc chương 1 đến chương 5,
bạn nên đọc trước: các cấu trúc dữ liệu được mô tả ở đó chính là công cụ mà ta
dùng ở đây. Các đoạn code trích dẫn được rút gọn nhẹ (bỏ bớt chỗ xuống dòng và
comment, và chỉ hiện một phía khi phía còn lại là bản đối xứng), nhưng hành vi
giống hệt mã nguồn.

## 6.1 API công khai

```rust
let mut engine = Engine::new(order_capacity);          // allocate everything, once
let mut events = Vec::new();                            // any EventSink (chapter 7)

engine.process_order(id, side, tick, qty, &mut events); // submit a limit order
let found = engine.cancel_order(id, &mut events);       // cancel a resting order: bool

engine.best_bid();                 // Option<u32>
engine.best_ask();                 // Option<u32>
engine.depth_at(side, tick);       // u64   total resting qty at one tick
engine.depth_snapshot(side, n);    // Vec<(tick, qty)>  best n levels, best first
engine.live_order_count();         // usize number of resting orders
engine.resting_order(id);          // Option<RestingOrder>  side, tick, remaining qty
```

Cả hai phương thức làm thay đổi sổ lệnh đều nhận event sink (nơi nhận sự kiện)
dưới dạng `sink: &mut S` với `S: EventSink + ?Sized`. Vì là generic nên trình
biên dịch tạo một bản riêng cho từng kiểu sink, và lời gọi `emit` được inline
thẳng vào vòng lặp khớp lệnh. Ràng buộc `?Sized` còn cho phép truyền
`&mut dyn EventSink` khi bạn muốn chỉ có một bản code được biên dịch.

| Phương thức | Thay đổi sổ? | Cấp phát? | Chi phí |
|---|---|---|---|
| `new(capacity)` | dựng sổ | **có**, một lần | O(capacity) |
| `process_order` | có | không (sink dạng `Vec` có thể cấp phát khi nó phình ra) | O(1 + số lần khớp) kỳ vọng |
| `cancel_order` | có | không | O(1) kỳ vọng |
| `best_bid` / `best_ask` | không | không | O(1), hai lần quét bit |
| `depth_at` | không | không | O(1) |
| `depth_snapshot` | không | **có** (`Vec`) | O(số tick phải quét); chỉ dùng để báo cáo |
| `live_order_count` | không | không | O(1) |
| `resting_order` | không | không | O(1) kỳ vọng: một lần tra bảng băm |

## 6.2 `process_order` gồm bốn giai đoạn

```mermaid
flowchart TD
    A(["process_order(id, side, tick, qty, sink)"]) --> B{"qty == 0, hoặc id bị dành riêng,<br/>hoặc tick >= 4096,<br/>hoặc id trùng với lệnh còn sống?"}
    B -- có --> R(["phát OrderRejected(reason)<br/>return ngay"])
    B -- không --> C["phát OrderAccepted"]
    C --> D["remaining = qty<br/>match_incoming với phía đối diện"]
    D --> E{"remaining == 0?"}
    E -- có --> F(["phát OrderFilled<br/>sequence += 1<br/>return"])
    E -- không --> G{"đã có giao dịch nào chưa?<br/>(filled > 0)"}
    G -- có --> H["phát OrderPartiallyFilled<br/>(taker, remaining_qty)"]
    G -- không --> I
    H --> I["cấp một ô arena cho phần còn lại"]
    I --> J{"còn ô trống không?"}
    J -- không --> K["phát OrderRejected(BookFull)"]
    J -- có --> L["id_map.insert<br/>push_back vào mức giá của chính nó"]
    L --> M{"push_back thành công?<br/>(không tràn số lượng)"}
    M -- không --> N["hoàn tác: id_map.remove, free ô<br/>phát OrderRejected(LevelQuantityOverflow)"]
    M -- có --> O["set bit của mức giá này trong bitset"]
    K --> P(["sequence += 1<br/>return"])
    N --> P
    O --> P
```

Tóm tắt bằng lời:

1. **Kiểm tra hợp lệ (validate).** Đầu vào sai bị từ chối trước khi có bất cứ
   thứ gì thay đổi.
2. **Chấp nhận (accept).** Phát sự kiện `OrderAccepted`.
3. **Khớp (match).** Giao dịch với phía đối diện chừng nào giá còn chạm nhau.
4. **Cho nằm chờ (rest).** Đưa phần số lượng còn lại vào sổ.

### Giai đoạn 1 và 2: kiểm tra hợp lệ và chấp nhận lệnh

```rust
fn validate(&self, order_id: u64, price_tick: u32, qty: u64) -> Result<(), RejectReason> {
    if qty == 0 {
        Err(RejectReason::ZeroQuantity)
    } else if order_id == crate::hashmap::RESERVED_KEY {
        Err(RejectReason::ReservedOrderId)
    } else if (price_tick as usize) >= MAX_TICKS {
        Err(RejectReason::PriceOutOfRange)
    } else if self.id_map.get(order_id).is_some() {
        Err(RejectReason::DuplicateOrderId)
    } else {
        Ok(())
    }
}

// In process_order:
if let Err(reason) = self.validate(order_id, price_tick, qty) {
    sink.emit(Event::OrderRejected { order_id, reason });
    return;
}
sink.emit(Event::OrderAccepted { order_id, side, price_tick, qty });
```

| Kiểm tra | `RejectReason` | Lý do |
|---|---|---|
| `qty == 0` | `ZeroQuantity` | Lệnh rỗng thì vô nghĩa. Nếu không có bước kiểm tra này, nó sẽ đi thẳng qua khâu khớp lệnh mà chẳng có gì để khớp, rồi phát ra một cặp `OrderAccepted` + `OrderFilled` vô lý. |
| `order_id == u64::MAX` | `ReservedOrderId` | Được dành riêng làm giá trị đánh dấu "trống" của bảng băm (chương 5). |
| `price_tick >= 4096` | `PriceOutOfRange` | Thang giá chỉ có 4096 tick; đánh chỉ mục vượt quá sẽ truy cập ngoài giới hạn mảng. |
| `id_map.get(id).is_some()` | `DuplicateOrderId` | Trùng id với một lệnh **còn sống** (live). (Id của lệnh đã khớp hết hoặc đã hủy thì không còn trong map, nên có thể dùng lại.) |

Khi kiểm tra thất bại, chỉ có **một** sự kiện `OrderRejected` được phát ra,
không có `OrderAccepted` đứng trước. Các bước kiểm tra chạy theo thứ tự trong
bảng và dừng ở lỗi đầu tiên, nên một lệnh mắc hai lỗi (ví dụ số lượng bằng 0
*và* tick sai) luôn nhận cùng một lý do là `ZeroQuantity`. Trả về `Result` từ
một hàm nhỏ giúp thứ tự đó dễ đọc, và để nơi gọi xử lý mọi trường hợp từ chối
chỉ bằng một câu `if let`.

### Giai đoạn 3: `match_incoming`, chọn phía

```rust
match side {
    Side::Buy  => Self::match_incoming(&mut self.pool, &mut self.ask_levels,
                      &mut self.ask_bitset, &mut self.id_map, &mut self.trades,
                      order_id, price_tick, &mut remaining, sink, Crossing::TakerBuy),
    Side::Sell => Self::match_incoming(&mut self.pool, &mut self.bid_levels,
                      &mut self.bid_bitset, &mut self.id_map, &mut self.trades,
                      order_id, price_tick, &mut remaining, sink, Crossing::TakerSell),
}
```

Một taker **mua** tấn công phía **ask**; một taker **bán** tấn công phía
**bid**. Cùng một hàm phục vụ cả hai, và chỉ có hai điểm khác nhau, được gói
trong enum `Crossing`:

| | `TakerBuy` | `TakerSell` |
|---|---|---|
| Giá tốt nhất phía đối diện | `bitset.lowest()` (ask rẻ nhất) | `bitset.highest()` (bid cao nhất) |
| Có thể khớp ngay khi | `tick <= limit_price` | `tick >= limit_price` |
| `taker_side` được báo trong `Trade` | `Side::Buy` | `Side::Sell` |

`self.trades` là bộ đếm giao dịch của engine: `match_incoming` tăng nó thêm một
sau mỗi giao dịch và ghi giá trị mới vào sự kiện làm `trade_id` của giao dịch đó
(6.10).

## 6.3 `match_incoming`, trái tim của engine

```mermaid
flowchart TD
    S(["match_incoming"]) --> A{"remaining == 0?"}
    A -- có --> Z(["return"])
    A -- không --> B["tick = giá tốt nhất phía đối diện<br/>(ask thấp nhất nếu là lệnh mua, bid cao nhất nếu là lệnh bán)"]
    B --> C{"phía đối diện<br/>còn mức giá nào không?"}
    C -- không --> Z
    C -- có --> D{"có thể khớp ngay?<br/>mua: tick <= limit<br/>bán: tick >= limit"}
    D -- không --> Z
    D -- có --> E["maker = đầu hàng đợi levels[tick]<br/>(lệnh cũ nhất ở mức giá đó)"]
    E --> F["trade_qty = min(remaining, maker.qty)<br/>phát Trade<br/>remaining -= trade_qty"]
    F --> G{"trade_qty == maker.qty?"}
    G -- có --> H["gỡ maker khỏi hàng đợi, free ô của nó,<br/>id_map.remove(maker)<br/>nếu mức giá giờ đã trống: bitset.clear(tick)<br/>phát OrderFilled(maker)"]
    G -- không --> I["maker.qty -= trade_qty, level.total -= trade_qty<br/>phát OrderPartiallyFilled(maker)"]
    H --> J{"remaining > 0 và<br/>mức giá vẫn còn lệnh?"}
    I --> J
    J -- có --> E
    J -- không --> K{"mức giá đã trống?<br/>(bit của nó đã được xóa)"}
    K -- có --> A
    K -- không --> Z
```

Có **hai vòng lặp lồng nhau**:

- **Vòng ngoài duyệt qua các mức giá**, mức tốt nhất trước.
- **Vòng trong duyệt hàng đợi FIFO ở một mức giá**, lệnh cũ nhất trước.

### Vòng ngoài: chọn giá tốt nhất và kiểm tra xem có chạm giá không

```rust
let Some(tick) = (match direction {
    Crossing::TakerBuy  => bitset.lowest(),
    Crossing::TakerSell => bitset.highest(),
}) else { break };                                   // opposite side is empty

let marketable = match direction {
    Crossing::TakerBuy  => tick as u32 <= price_tick,
    Crossing::TakerSell => tick as u32 >= price_tick,
};
if !marketable { break; }
```

Câu `break` sớm khi giá tốt nhất không thể khớp ngay vừa là một tối ưu quan
trọng, vừa là một lập luận chứng minh tính đúng đắn: nếu giá *tốt nhất* phía đối
diện còn không chạm tới giới hạn, thì **mọi giá khác phía đối diện đều tệ hơn**,
nên không thể có thêm giao dịch nào. Không cần quét thêm.

### Vòng trong: khớp với lệnh ở đầu hàng đợi

```rust
let maker_idx       = level.head;                      // oldest order at this tick
let maker_qty       = pool.get(maker_idx).remaining_qty;
let maker_order_id  = pool.get(maker_idx).order_id;
let trade_qty       = (*remaining).min(maker_qty);     // the smaller of the two

*trades += 1;                                          // number this trade
sink.emit(Event::Trade { trade_id: *trades, maker_order_id, taker_order_id,
                         taker_side, price_tick: tick as u32, qty: trade_qty });
*remaining -= trade_qty;

if trade_qty == maker_qty {
    level.remove(pool, maker_idx);          // maker fully consumed
    pool.free(maker_idx);
    id_map.remove(maker_order_id);
    if level.is_empty() {
        bitset.clear(tick);                 // price gone: update the index first...
    }
    sink.emit(Event::OrderFilled { order_id: maker_order_id }); // ...then report
} else {
    level.reduce_qty(pool, maker_idx, trade_qty);   // maker keeps its place
    sink.emit(Event::OrderPartiallyFilled {
        order_id: maker_order_id,
        remaining_qty: maker_qty - trade_qty,
    });
}
```

Mỗi bước có ba kết quả có thể xảy ra, tùy theo
`trade_qty = min(taker_left, maker_qty)`:

| Trường hợp | Kết quả |
|---|---|
| taker_left **>** maker_qty | Maker khớp hết và bị gỡ khỏi sổ. Taker lặp tiếp. |
| taker_left **<** maker_qty | Taker khớp hết (`remaining` về 0). Maker vẫn ở lại, với số lượng đã giảm. |
| taker_left **==** maker_qty | Cả hai cùng khớp hết. |

Hãy để ý: **giá giao dịch là `tick`, tức mức giá của maker**, chứ không phải
giới hạn của taker. Nhờ vậy giá tốt hơn mong đợi (price improvement, chương 1)
có sẵn mà không tốn thêm công sức.

### Sau vòng lặp trong

```rust
if !levels[tick].is_empty() {
    break;                  // the taker ran out first: nothing more can trade
}
// The level drained; its bit was cleared with its last maker. Loop for the next price.
```

Nếu mức giá vẫn còn lệnh, thì taker chắc chắn đã hết số lượng, nên ta dừng.
Nếu mức giá đã cạn, quyền điều khiển quay về vòng ngoài, và vòng ngoài hỏi
bitset về mức giá tốt nhất *tiếp theo*.

> [!NOTE]
> **Tại sao phải xóa bit ngay trong vòng lặp trong, trước khi báo lệnh khớp?**
> Nếu xóa sau vòng lặp thì việc khớp lệnh vẫn đúng như nhau. Nhưng engine đưa
> sự kiện `OrderFilled` của maker cho sink của nơi gọi, và đó là code mà engine
> không kiểm soát. Quy tắc là: **chỉ gọi ra sink khi mọi cấu trúc dữ liệu đã
> nhất quán với nhau** (bất biến 6 ở mục 6.9). Nếu bit vẫn đang bật, `best_ask()`
> sẽ báo một mức giá mà thực ra không còn lệnh nào. Lỗi này chỉ lộ ra khi một
> sink bị panic và panic đó bị bắt lại, nhưng chính những quy tắc như thế giúp
> codebase không có các lỗi kiểu "chỉ lộ ra khi...". Unit test
> `panicking_sink_leaves_the_book_consistent` khóa chặt hành vi này.

## 6.4 Giai đoạn 4: cho phần còn lại nằm chờ trong sổ

(Đoạn trích đã rút gọn: nhánh `Sell` đối xứng với nhánh `Buy`.)

```rust
match self.pool.allocate(order_id, side, price_tick, remaining, self.sequence) {
    Some(idx) => {
        self.id_map.insert(order_id, idx);
        let rested = match side {
            Side::Buy  => { let ok = self.bid_levels[price_tick as usize].push_back(&mut self.pool, idx);
                            if ok { self.bid_bitset.set(price_tick as usize); } ok }
            Side::Sell => { /* same with ask_levels / ask_bitset */ }
        };
        if !rested {
            self.id_map.remove(order_id);   // roll back
            self.pool.free(idx);
            sink.emit(Event::OrderRejected { order_id, reason: RejectReason::LevelQuantityOverflow });
        }
    }
    None => sink.emit(Event::OrderRejected { order_id, reason: RejectReason::BookFull }),
}
```

Có hai nguyên nhân khiến một lệnh không thể nằm chờ trong sổ, và cả hai đều tạo
ra `OrderRejected` **sau** `OrderAccepted`:

| Nguyên nhân | `RejectReason` | Lý do | Dọn dẹp |
|---|---|---|---|
| Arena đầy (`allocate` trả về `None`) | `BookFull` | đã có `order_capacity` lệnh đang nằm chờ | không có gì để hoàn tác |
| Tổng số lượng của mức giá bị tràn (`push_back` trả về `false`) | `LevelQuantityOverflow` | `total_qty` sẽ vượt quá `u64::MAX` | xóa id khỏi `id_map`, free ô |

> [!IMPORTANT]
> **Trên thực tế, việc từ chối vì hết dung lượng chỉ xảy ra với lệnh chưa giao
> dịch lần nào.** Comment trong mã nguồn nói rằng các giao dịch đã thực hiện sẽ
> không bị hoàn tác, và đường code được viết như vậy để phòng thủ. Nhưng tình
> huống đó không thể xảy ra:
>
> - *Arena đầy:* taker vẫn còn số lượng sau khi khớp thì chắc chắn đã khớp hết
>   mọi maker mà nó chạm tới, và mỗi maker khớp hết đều **giải phóng một ô**.
>   Vì vậy `allocate` luôn thành công sau ít nhất một giao dịch.
> - *Tràn mức giá:* sổ không bao giờ bị chéo giá (crossed, xem 6.9). Nếu taker
>   đã giao dịch, thì phía của chính nó ở mức giá đó không có lệnh nào đang chờ,
>   nên mức giá đang trống và `total_qty` của nó không thể tràn.
>
> Vì vậy chuỗi sự kiện từ chối thực sự có thể xảy ra chỉ là
> `[OrderAccepted, OrderRejected]`. Kiểm thử đối chiếu (differential test)
> trong `tests/model_check.rs` kiểm tra điều này trên từng lời gọi trong số
> 160,000 lời gọi ngẫu nhiên của nó, nhiều lời gọi chạy trên arena chỉ có 64
> lệnh và đầy gần như suốt thời gian: lời gọi nào kết thúc bằng `OrderRejected`
> sau `OrderAccepted` thì không được chứa `Trade` nào. Bạn chạy nó bằng
> `cargo test --test model_check`. Nếu bạn sửa engine để cả hai có thể xảy ra
> cùng lúc, câu assert đó sẽ thất bại, và khi đó bên tiêu thụ sự kiện phải coi
> các giao dịch kia là thật.

## 6.5 Theo dõi trọn vẹn từng bước

Ta hãy chạy lại bước 6 của
[ví dụ minh họa](01-order-book-basics.md#110-một-ví-dụ-hoàn-chỉnh) và quan
sát từng cấu trúc dữ liệu thay đổi ra sao. Trạng thái ban đầu (dung lượng 16;
xem [chương 2](02-architecture.md#các-mảnh-ghép-trỏ-vào-nhau-như-thế-nào) để có
bức tranh đầy đủ):

```text
 ask:   [101] #2(ô1):3 -> #3(ô2):4   tổng 7        ask_bitset {101, 102}
        [102] #1(ô0):5               tổng 5
 bid:   [100] #5(ô4):6                             bid_bitset {99, 100}
        [ 99] #4(ô3):10
 đỉnh free stack -> ô 5, 6, 7, ...
```

**Lệnh đến:** `process_order(6, Buy, tick 102, qty 14)`

| # | Điều gì xảy ra | Cấu trúc dữ liệu thay đổi | Sự kiện được phát | `remaining` |
|---|---|---|---|---|
| 0 | Kiểm tra hợp lệ: qty>0, id≠MAX, 102<4096, id 6 chưa dùng. | | `OrderAccepted{6, Buy, 102, 14}` | 14 |
| 1 | `ask_bitset.lowest()` = **101**. `101 <= 102` ✓ có thể khớp ngay. | | | 14 |
| 2 | Phần tử đầu của `ask[101]` là ô 1 (`#2`, qty 3). `trade_qty = min(14,3) = 3`. | | `Trade{id 1, maker 2, taker 6, Buy, 101, 3}` | 11 |
| 3 | 3 == 3, nên `#2` khớp hết. | `ask[101]`: head → ô 2, count 1, total 4. `free(slot 1)`. `id_map.remove(2)`. | `OrderFilled{2}` | 11 |
| 4 | Mức 101 chưa trống. Phần tử đầu là ô 2 (`#3`, qty 4). `trade_qty = min(11,4) = 4`. | | `Trade{id 2, maker 3, taker 6, Buy, 101, 4}` | 7 |
| 5 | 4 == 4, nên `#3` khớp hết. Mức giá giờ đã trống, nên bit của nó được xóa trước khi phát sự kiện. | `ask[101]` trống (head = tail = NIL, count 0, total 0). `free(slot 2)`. `id_map.remove(3)`. `ask_bitset.clear(101)`, giờ là `{102}`. | `OrderFilled{3}` | 7 |
| 6 | Vòng trong kết thúc (mức giá trống, bit đã được xóa). | | | 7 |
| 7 | Vòng ngoài: `lowest()` = **102**. `102 <= 102` ✓. Phần tử đầu của `ask[102]` là ô 0 (`#1`, qty 5). `trade_qty = min(7,5) = 5`. | | `Trade{id 3, maker 1, taker 6, Buy, 102, 5}` | 2 |
| 8 | 5 == 5, nên `#1` khớp hết; mức giá của nó cũng trống theo. | `ask[102]` trống. `free(slot 0)`. `id_map.remove(1)`. `ask_bitset.clear(102)`, giờ rỗng. | `OrderFilled{1}` | 2 |
| 9 | Vòng trong kết thúc (mức giá trống, bit đã được xóa). | | | 2 |
| 10 | Vòng ngoài: `lowest()` = `None` → dừng. | | | 2 |
| 11 | `filled = 14 − 2 = 12 > 0`, `remaining = 2 ≠ 0`. | | `OrderPartiallyFilled{6, remaining 2}` | 2 |
| 12 | Cho phần còn lại nằm chờ: `allocate` lấy ra ô vừa được giải phóng gần nhất, **ô 0**. | `id_map.insert(6, 0)`. `bid[102]`: head = tail = ô 0, count 1, total 2. `bid_bitset.set(102)`. `sequence = 6`. | | 2 |

Trạng thái cuối:

```text
 ask:   (trống)                                    ask_bitset {}
 bid:   [102] #6(ô0):2                             bid_bitset {99, 100, 102}
        [100] #5(ô4):6
        [ 99] #4(ô3):10
 best_bid = 102, best_ask = None, live_order_count = 3
```

Mỗi bước ở trên đều là O(1), và không có byte bộ nhớ nào được cấp phát.

### Thêm vài ví dụ (đều do engine thật tạo ra)

**Giá tốt hơn mong đợi** (price improvement): một lệnh mua giới hạn ở giá 105
gặp một ask ở giá 100 sẽ giao dịch ở giá 100:

```text
 process_order(1, Sell, 100, 5)
 process_order(2, Buy, 105, 5)   ->  OrderAccepted{2, Buy, price_tick 105, qty 5}
                                     Trade{id 1, maker 1, taker 2, Buy, price_tick 100, qty 5}
                                     OrderFilled{1}
                                     OrderFilled{2}
```

**FIFO trong cùng một mức giá** (lấy từ unit test
`price_time_priority_within_a_level_is_fifo`): hai ask, mỗi lệnh số lượng 5 ở
tick 100 (`#1` rồi đến `#2`), và một lệnh mua 6. `#1` giao dịch trước (5), rồi
đến `#2` (1).

## 6.6 `cancel_order`

```rust
pub fn cancel_order<S: EventSink + ?Sized>(&mut self, order_id: u64, sink: &mut S) -> bool {
    let Some(idx) = self.id_map.remove(order_id) else {
        sink.emit(Event::CancelRejected { order_id });   // nothing to cancel: say so
        return false;
    };
    let order = *self.pool.get(idx);                     // copy it out (Order is Copy)
    match order.side {
        Side::Buy => {
            let level = &mut self.bid_levels[order.price_tick as usize];
            level.remove(&mut self.pool, idx);           // unlink from the FIFO
            if level.is_empty() { self.bid_bitset.clear(order.price_tick as usize); }
        }
        Side::Sell => { /* same on the ask side */ }
    }
    self.pool.free(idx);
    sink.emit(Event::OrderCancelled {                    // every structure agrees by now
        order_id, side: order.side, price_tick: order.price_tick,
        cancelled_qty: order.remaining_qty,
    });
    true
}
```

| Bước | Cấu trúc được dùng | Chi phí |
|---|---|---|
| Tìm ô chứa lệnh | `id_map.remove(order_id)` | O(1) kỳ vọng |
| Đọc phía (mua/bán) và tick | `pool.get(idx)` | O(1) |
| Gỡ khỏi hàng đợi (ở đầu, giữa hoặc cuối) | `PriceLevel::remove` | O(1) |
| Bỏ mức giá khỏi chỉ mục nếu giờ đã trống | `bitset.clear` | O(1) |
| Trả ô về để dùng lại | `pool.free` | O(1) |

Hủy một id **không tồn tại** hoặc **đã khớp hết** sẽ không đụng gì đến sổ, phát
`CancelRejected` và trả về `false`. Việc phát một sự kiện, chứ không chỉ trả về
`false`, có ý nghĩa với bất kỳ ai theo dõi engine chỉ qua các sự kiện của nó: mọi
yêu cầu hủy đều để lại đúng một sự kiện. (Hãy thử hủy lệnh 4 hai lần trong ví dụ
minh họa: lần đầu trả về `true` kèm `OrderCancelled`, lần sau trả về `false` kèm
`CancelRejected`.)

`OrderCancelled` sao chép phía, tick và số lượng còn lại ra khỏi bản ghi lệnh
trước khi ô được free, nhờ vậy bên tiêu thụ biết mức giá nào bị mất bao nhiêu. Để
đọc chính các field đó *mà không* hủy lệnh, hãy dùng `resting_order(id)`: cùng
một lần tra `id_map`, rồi sao chép các field của lệnh vào một `RestingOrder`.

## 6.7 Các trường hợp biên, đã được kiểm chứng

| Đầu vào | Sự kiện được phát | Ảnh hưởng đến sổ |
|---|---|---|
| `qty = 0` | `[OrderRejected(ZeroQuantity)]` | không có |
| `price_tick = 4096` (ngoài phạm vi) | `[OrderRejected(PriceOutOfRange)]` | không có |
| `price_tick = 4095` (tick hợp lệ cuối cùng) | `[OrderAccepted]` (nếu không có gì khớp) | nằm chờ bình thường |
| `order_id = u64::MAX` | `[OrderRejected(ReservedOrderId)]` | không có |
| Trùng id với một lệnh còn sống | `[OrderRejected(DuplicateOrderId)]` | không có |
| Dùng lại id của một lệnh đã khớp hết/đã hủy | xử lý như mọi lệnh mới | bình thường |
| Lệnh phải nằm chờ nhưng arena đã đầy | `[OrderAccepted, OrderRejected(BookFull)]` (không bao giờ có giao dịch đi trước, xem 6.4) | không có gì được đưa vào sổ |
| Lệnh phải nằm chờ nhưng tổng số lượng của mức giá sẽ tràn `u64` | `[OrderAccepted, OrderRejected(LevelQuantityOverflow)]` | không có gì được đưa vào sổ; trạng thái không đổi |
| Lệnh khớp hết hoàn toàn khi arena đã đầy | các lần khớp bình thường | không cần ô nào, nên vẫn thành công |
| `cancel_order` với một id không nằm chờ trong sổ | `[CancelRejected]` (trả về `false`) | không có |

## 6.8 Độ phức tạp

- **Giai đoạn 1 và 2 (kiểm tra, chấp nhận):** O(1) kỳ vọng: một lần tra bảng
  băm.
- **Giai đoạn 3 (khớp):** mỗi vòng lặp tạo ra một giao dịch, rồi hoặc loại một
  maker khỏi sổ (`O(1)` cho gỡ khỏi hàng đợi + free + xóa khỏi bảng băm), hoặc
  làm taker hết số lượng. Chuyển sang mức giá kế tiếp là một lần quét bit, cũng
  O(1). Mỗi mức giá được duyệt qua đều tạo ra ít nhất một giao dịch, nên tổng
  chi phí là **O(số lần khớp)**.
- **Giai đoạn 4 (nằm chờ):** O(1).

Tổng thể: **O(1 + số lần khớp) kỳ vọng** (chữ "kỳ vọng" có mặt vì bảng băm phải
dò; xem chương 5), khớp với doc comment ("O(1) + O(fills)"). Một taker không
vượt spread có chi phí gần như bằng mọi lệnh không khớp khác, bất kể trong sổ có
bao nhiêu lệnh.

## 6.9 Các bất biến sau mỗi lần gọi

Các bất biến (invariant) này luôn đúng giữa các lần gọi, và cái cuối cùng còn
đúng ngay cả trong lúc đang gọi. Kiểm thử đối chiếu ở chương 10
(`tests/model_check.rs`) kiểm tra những cái quan sát được (giá tốt nhất, độ sâu,
số lệnh còn sống, chính xác từng sự kiện) sau mỗi yêu cầu:

1. **Sổ lệnh không bao giờ bị chéo giá (crossed):** `best_bid < best_ask` mỗi
   khi cả hai cùng tồn tại.
   *Vì sao:* một lệnh mua đến ở giá `p` sẽ khớp hết mọi ask `≤ p` trước khi nằm
   chờ, nên ask nào còn lại đều `> p`; với lệnh bán thì đối xứng.
2. **Bit `t` của bitset ở một phía được bật ⇔ `levels[t]` của phía đó không
   rỗng.**
3. **Mỗi lệnh đang chờ nằm trong đúng một hàng đợi FIFO và có đúng một mục
   trong `id_map`;** `live_order_count()` đếm chúng.
4. **`level.total_qty` bằng tổng `remaining_qty` của các lệnh trong mức giá
   đó.**
5. **Trong một hàng đợi, thứ tự đến được giữ nguyên:** lệnh mới nối vào đuôi,
   và maker mới khớp một phần vẫn giữ vị trí đầu hàng.
6. **Sổ luôn nhất quán mỗi khi sink chạy**, chứ không chỉ giữa các lần gọi.
   Mỗi lần `emit` đều xảy ra giữa hai thay đổi trạng thái hoàn chỉnh, và đó là
   lý do một mức giá đã cạn rời khỏi bitset trước khi lệnh khớp của maker cuối
   cùng được báo (6.3). Nhờ vậy sink không bao giờ nhìn thấy một sổ mới cập nhật
   dở. Nếu một sink bị panic và panic đó bị bắt, sổ vẫn nhất quán, nhưng yêu
   cầu đó mới chỉ được áp dụng một phần (chương 7 giải thích khi đó nên làm
   gì).

## 6.10 Những ghi chú thiết kế đáng chú ý

**Một hàm cho cả hai phía.** Thay vì nhân đôi vòng lặp khớp lệnh cho lệnh mua và
lệnh bán, `match_incoming` nhận vùng lưu trữ của phía đối diện cùng một giá trị
`Crossing`. Các nhánh `match direction` cực kỳ dễ cho CPU dự đoán (branch
prediction).

**Vì sao `match_incoming` là associated function (hàm liên kết), không phải
phương thức `&mut self`.** Nó nhận `&mut self.pool`, `&mut self.ask_levels`,
`&mut self.ask_bitset` và `&mut self.id_map` làm các tham số riêng. Rust cho
phép mượn dạng mutable *các field rời nhau* (disjoint fields) của một struct
cùng một lúc, nhưng không cho mượn cả struct cộng thêm một field: một phương
thức `fn match_incoming(&mut self, levels: &mut [PriceLevel], ...)` sẽ không
thể được gọi với `&mut self.ask_levels`, vì `self` đã bị mượn nguyên cả khối
rồi.

Thật ra phương thức `&mut self` *vẫn làm được*: nó có thể mở đầu bằng
`let (levels, bitset) = match direction { ... }` rồi tự chọn các field, vì
trong thân một hàm thì mượn hai field khác nhau là được phép. Cách dùng
associated function được chọn vì nó giữ quyết định đó ở nơi gọi, và làm cho mọi
phần trạng thái mà vòng lặp đụng tới đều lộ ra ngay trong chữ ký hàm.
`#[allow(clippy::too_many_arguments)]` là cái giá phải trả cho sự rõ ràng ấy.

**Bộ đếm `sequence`.** Nó tăng một lần cho mỗi lệnh vượt qua bước kiểm tra hợp
lệ, và được sao chép vào bản ghi `Order` nếu lệnh đó nằm chờ trong sổ. Engine
không bao giờ *đọc lại* giá trị này: ưu tiên về thời gian đến từ **vị trí trong
danh sách liên kết**, chứ không phải từ việc so sánh các số sequence. Nó chỉ là
metadata phục vụ kiểm toán (audit) hoặc gỡ lỗi. (Nó dùng `wrapping_add` nên
không bao giờ panic.)

**Bộ đếm `trades`.** Khác với `sequence`, bộ đếm này có được đọc: nó *chính là*
`trade_id` của giao dịch gần nhất. Nó chỉ thay đổi bên trong vòng lặp khớp lệnh,
vốn có tính tất định (deterministic), nên khi chạy lại (replay) cùng các yêu cầu
đó thì mọi giao dịch đều được đánh số y như cũ, và phía sau có thể dùng các id
này để phát hiện bản trùng lặp và chỗ bị hụt (chương 7). Một số `u64` tăng thêm
một cho mỗi giao dịch thì trên thực tế không bao giờ tràn, nên nó dùng `+= 1`
bình thường.

**`#[inline(always)]` trên các accessor nhỏ xíu** (`best_bid`, `best_ask`,
`live_order_count`, các accessor của bitset và pool) loại bỏ chi phí gọi hàm
trong hot path.

## 6.11 Tự thực hành

Chạy mọi test của engine, rồi chạy kiểm thử đối chiếu so engine với một sổ lệnh
tham chiếu được viết cố ý thật đơn giản, trên 160,000 lời gọi ngẫu nhiên:

```sh
cargo test engine::
cargo test --test model_check
```

Để xem bản trace ở mục 6.5 diễn ra ngay trước mắt, hãy chạy ví dụ của chương 1:

```sh
cargo run --example walkthrough
```

Sau đó hãy cố tình làm hỏng một chỗ. Trong `match_incoming` ở
[`src/engine.rs`](../../src/engine.rs), đổi
`Crossing::TakerBuy => tick as u32 <= price_tick` thành
`Crossing::TakerBuy => (tick as u32) < price_tick`, để lệnh mua không còn giao
dịch ở đúng mức giá giới hạn của nó nữa. (Cần có dấu ngoặc: nếu thiếu, Rust sẽ
hiểu `u32 <` là phần mở đầu của một kiểu generic như `u32<...>`.) Hãy đoán
trước test nào sẽ thất bại, chạy `cargo test`, rồi xem bạn đoán đúng không.
(Nhớ hoàn tác thay đổi sau khi làm xong.)

**Tiếp theo:** [7. Sự kiện (events)](07-events.md)
