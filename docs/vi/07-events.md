# 7. Sự kiện (events): bộ khớp lệnh báo cáo những gì đã xảy ra

> *Bản gốc tiếng Anh: [07-events.md](../07-events.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Mã nguồn: [`src/events.rs`](../../src/events.rs)

Bộ khớp lệnh không in dữ liệu, không ghi vào socket và không quyết định nơi
nhận kết quả xử lý. Mỗi lần gọi `process_order` hoặc `cancel_order` đều nhận thêm một
tham số, gọi là **event sink** (bộ nhận sự kiện), và bộ khớp lệnh gửi từng sự kiện đến
sink ngay khi sự kiện phát sinh. Bên gọi tự quyết định làm gì với chúng (phát dữ
liệu thị trường, cập nhật tài khoản, ghi nhật ký...).

Nhờ vậy bộ khớp lệnh là một máy trạng thái (state machine) thuần túy và tất định
(deterministic):

```text
   (trạng thái sổ, yêu cầu)  --->  Engine  --->  (trạng thái sổ mới, các sự kiện đưa vào sink)
```

## 7.1 Enum `Event`

```rust
pub enum Event {
    OrderAccepted        { order_id: u64, account: u32, side: Side,
                           order_type: OrderType, price_tick: u32, qty: u64 },
    Trade                { trade_id: u64, maker_order_id: u64, taker_order_id: u64,
                           taker_side: Side, price_tick: u32, qty: u64 },
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    OrderFilled          { order_id: u64 },
    OrderExpired         { order_id: u64, expired_qty: u64 },
    OrderCancelled       { order_id: u64, side: Side, price_tick: u32, cancelled_qty: u64 },
    OrderReduced         { order_id: u64, side: Side, price_tick: u32,
                           reduced_qty: u64, remaining_qty: u64 },
    OrderRejected        { order_id: u64, reason: RejectReason },
    SelfTradePrevented   { taker_order_id: u64, maker_order_id: u64 },
    MassCancelled        { account: Option<u32>, cancelled_orders: u64 },
    CancelRejected       { order_id: u64, reason: CancelRejectReason },
}

pub enum RejectReason {
    // Found before acceptance: the order is rejected alone, the book unchanged.
    ZeroQuantity,          // qty was 0
    ReservedOrderId,       // order_id was u64::MAX
    PriceOutOfRange,       // price_tick was not below the engine's tick count
    DuplicateOrderId,      // a resting order already has this id
    QuantityAboveLimit,    // qty was above the risk limit max_order_qty
    PriceOutsideBand,      // the price reached past the risk limit price_band
    PostOnlyWouldTrade,    // a post-only order's price reaches the other side
    // The remainder could not rest: always after OrderAccepted.
    BookFull,              // every arena slot is in use
    LevelQuantityOverflow, // the level's total would not fit in a u64
}

pub enum CancelRejectReason {
    NotResting,            // no resting order has this id
    ZeroQuantity,          // a reduce of 0
}
```

| Sự kiện | Được phát ra khi | Đối tượng liên quan |
|---|---|---|
| `OrderAccepted` | Một lệnh mới đã qua bước kiểm tra hợp lệ. Sự kiện chứa tài khoản, phía, loại lệnh, giá giới hạn và số lượng của lệnh (với lệnh thị trường, đó là giá giới hạn mà nó được gán: mép của biên độ giá (price band), hoặc mức giá tệ nhất mà phía của nó cho phép). | lệnh vừa đến |
| `Trade` | Hai lệnh khớp với nhau. `taker_side` là phía của bên chủ động (aggressor); `trade_id` đánh số các giao dịch 1, 2, 3, ... | cả maker **và** taker, ở giá của maker |
| `OrderPartiallyFilled` | Một lệnh đã giao dịch nhưng vẫn còn số lượng. | một maker (sau mỗi lần khớp một phần), hoặc taker (một lần, ở cuối) |
| `OrderFilled` | Một lệnh đã khớp hết số lượng. | một maker (khi nó bị khớp hết), hoặc taker |
| `OrderExpired` | Phần còn lại của một lệnh immediate-or-cancel hoặc lệnh thị trường bị bỏ đi; nó chưa bao giờ vào sổ. | taker (một lần ở cuối) |
| `OrderCancelled` | Một lệnh đang chờ bị hủy theo yêu cầu (hoặc bị giảm đi toàn bộ số lượng còn lại). Sự kiện cho biết lệnh đã nằm chờ ở đâu và số lượng bị loại khỏi sổ. | lệnh bị hủy |
| `OrderReduced` | Số lượng của một lệnh đang chờ được giảm theo yêu cầu; lệnh vẫn giữ nguyên vị trí trong hàng đợi. | lệnh bị giảm số lượng |
| `OrderRejected` | Một lệnh bị từ chối: đầu vào sai, vi phạm một giới hạn rủi ro, một lệnh post-only sẽ gây ra giao dịch, hoặc không còn chỗ để nằm chờ. `reason` cho biết lý do. | lệnh vừa đến |
| `SelfTradePrevented` | Một lệnh mới sắp khớp với lệnh đang chờ của cùng tài khoản, nhưng chế độ chống tự giao dịch (self-trade prevention) đã chặn giao dịch. Theo sau là `OrderCancelled` của maker, `OrderExpired` của taker, hoặc cả hai, tùy theo chế độ. | cả taker **và** maker |
| `MassCancelled` | Một lần gọi `cancel_account` (`account: Some(..)`) hoặc `cancel_all` (`None`) đã hoàn tất; `cancelled_orders` đếm số sự kiện `OrderCancelled` đứng trước nó. | yêu cầu đó |
| `CancelRejected` | Một yêu cầu hủy hoặc giảm lệnh bị từ chối: không có lệnh đang chờ nào mang ID đó, hoặc lượng giảm bằng 0. `reason` cho biết lý do. | ID trong yêu cầu |

`Event` dùng `derive` để triển khai `Debug, Clone, Copy, PartialEq, Eq`, nên
việc sao chép ít tốn chi phí và có thể dễ dàng so sánh sự kiện trong kiểm thử. Một `Event` chiếm 40 byte (biến thể lớn nhất là
`Trade`, chứa `8 + 8 + 8 + 8 + 4 + 1 = 37` byte, cộng với tag, rồi làm tròn lên bội
số của 8). Các biến thể khác đều nằm vừa trong vùng nhớ dành cho `Trade`.

## 7.2 Quy tắc về thứ tự

Với mỗi lần gọi `submit_order` (hoặc `process_order`), các sự kiện luôn có dạng
sau:

```text
   [OrderAccepted]                                   <- luôn đứng đầu (trừ khi bị từ chối ngay từ đầu)
   ( Trade, <maker Filled | maker PartiallyFilled> ) <- lặp lại mỗi lần khớp, theo đúng thứ tự khớp
   [ taker OrderFilled | taker OrderPartiallyFilled ]<- nhiều nhất một sự kiện, cho biết trạng thái cuối của taker
   [ OrderExpired ]                                  <- lệnh IOC và lệnh thị trường còn số lượng, hoặc một lần tự giao dịch bị chặn
   [ OrderRejected ]                                 <- chỉ khi lệnh không nằm chờ được (và chưa giao dịch lần nào)
```

Những điểm cần nhớ:

- Ngay sau một `Trade` **luôn là** sự kiện mô tả trạng thái mới của
  *maker*.
- Khi có chống tự giao dịch, một `SelfTradePrevented` thay cho một `Trade`
  trong chuỗi đó. Theo sau nó là `OrderCancelled` của chính maker cùng tài khoản
  (chế độ `CancelResting` và `CancelBoth`), và với `CancelIncoming` và
  `CancelBoth` thì việc khớp lệnh dừng lại tại đó: sự kiện cuối của taker và
  `OrderExpired` đến ngay sau, kể cả với lệnh giới hạn.
- `OrderPartiallyFilled` hoặc `OrderFilled` của **taker** chỉ xuất hiện **một
  lần**, ở cuối, chứ không xuất hiện sau mỗi giao dịch. Muốn theo dõi tiến độ của
  một taker, hãy cộng `qty` của các sự kiện `Trade` của nó lại.
- Một taker không giao dịch lần nào mà chỉ nằm chờ thì chỉ tạo ra **duy nhất**
  `OrderAccepted`. Không có sự kiện riêng kiểu "lệnh đang nằm chờ".
- Lệnh bị từ chối trước khi được chấp nhận (đầu vào sai, vi phạm một giới hạn rủi ro, hoặc lệnh post-only
  có thể khớp ngay) thì chỉ phát **một sự kiện từ chối**; lệnh bị từ chối vì hết dung
  lượng thì sự kiện từ chối xuất hiện **sau** `OrderAccepted`, và chỉ với lệnh chưa
  giao dịch lần nào (xem
  [mục 6.4](06-matching-algorithm.md#64-giai-đoạn-4-cho-phần-còn-lại-nằm-chờ-trong-sổ)).
- Bản thân bộ khớp lệnh **không bao giờ bỏ sót sự kiện**. Chỉ có sink có giới hạn, như
  `EventBatch`, mới có thể hết chỗ (mục 7.4).
- Một lần gọi `cancel_order` hoặc `reduce_order` phát ra **đúng một** sự kiện:
  `OrderCancelled` hoặc `OrderReduced` nếu nó làm thay đổi sổ lệnh,
  `CancelRejected` nếu không. Kết quả `bool` của nó cũng cho biết đúng điều đó,
  dành cho bên gọi chỉ muốn rẽ nhánh theo kết quả.
- Một lần gọi `cancel_account` hoặc `cancel_all` phát ra một `OrderCancelled`
  cho mỗi lệnh, phía bid trước (giá tốt nhất trước), rồi đến phía ask (giá tốt
  nhất trước), lệnh cũ nhất trước ở mỗi mức giá, và luôn kết thúc bằng
  `MassCancelled`.

### Theo dõi sổ lệnh chỉ bằng sự kiện

Các sự kiện mang đủ thông tin để duy trì một bản sao độ sâu của sổ lệnh mà không cần
đọc các yêu cầu đầu vào. Đó chính là việc mà một hệ thống phát dữ liệu thị trường
hay một hệ thống quản lý rủi ro làm:

| Sự kiện | Thay đổi trên bản sao |
|---|---|
| `OrderAccepted { side, price_tick, qty, .. }` | ghi nhớ các thông số này cho đến khi lần gọi kết thúc |
| `Trade { taker_side, price_tick, qty, .. }` | mức giá của maker mất đi `qty`; mức đó nằm ở phía **đối diện** với `taker_side`, tại `price_tick` |
| `OrderCancelled { side, price_tick, cancelled_qty, .. }` | mức giá đó mất đi `cancelled_qty` |
| `OrderReduced { side, price_tick, reduced_qty, .. }` | mức giá đó mất đi `reduced_qty` |
| `SelfTradePrevented`, `MassCancelled` | không có gì: các sự kiện `OrderCancelled` quanh chúng mang theo các thay đổi |
| kết thúc một lần gọi `submit_order` | trừ khi sự kiện cuối cùng của taker là `OrderFilled`, `OrderExpired` hoặc `OrderRejected`, phần còn lại của nó (`qty` trừ đi các giao dịch của nó) giờ nằm chờ tại chính `price_tick` của nó |

`trade_id` giúp xử lý các giao dịch bị thiếu hoặc bị gửi lặp nói ở
[mục 9.5](09-write-ahead-log.md#95-đọc-và-phát-lại): bộ khớp lệnh đánh số các giao dịch
theo đúng thứ tự chúng xảy ra, và bộ khớp lệnh là tất định, nên phát lại cùng các yêu cầu
đó sẽ gán đúng các ID như trước. Một bên tiêu thụ đã thấy giao dịch 41 có thể bỏ
qua mọi giao dịch có ID nhỏ hơn, còn một bước nhảy từ 41 lên 43 nghĩa là nó đã bỏ
lỡ một giao dịch.

### Các chuỗi sự kiện mẫu

Các chuỗi này được lấy trực tiếp từ kiểm thử đơn vị trong `engine.rs`.

| Tình huống | Sự kiện |
|---|---|
| Lệnh nằm chờ, không có gì khớp | `Accepted` |
| Khớp vừa đủ, taker bằng maker | `Accepted`, `Trade`, `Filled(maker)`, `Filled(taker)` |
| Taker nhỏ hơn maker | `Accepted`, `Trade`, `PartiallyFilled(maker, left)`, `Filled(taker)` |
| Taker lớn hơn maker; phần còn lại nằm chờ | `Accepted`, `Trade`, `Filled(maker)`, `PartiallyFilled(taker, left)` |
| Taker quét qua nhiều mức giá | `Accepted`, rồi mỗi lần khớp có `Trade` + sự kiện của maker, cuối cùng là sự kiện cuối của taker |
| Đầu vào không hợp lệ / trùng ID | `Rejected(ZeroQuantity / ReservedOrderId / PriceOutOfRange / DuplicateOrderId)` |
| Lệnh có số lượng vượt mức tối đa, hoặc có giá vượt ra ngoài biên độ giá | `Rejected(QuantityAboveLimit / PriceOutsideBand)` |
| Lệnh post-only có giá chạm tới phía đối diện | `Rejected(PostOnlyWouldTrade)` |
| Lệnh không nằm chờ được (arena đầy / tràn số) | `Accepted`, `Rejected(BookFull / LevelQuantityOverflow)` (chưa có giao dịch nào) |
| Lệnh IOC hoặc lệnh thị trường còn số lượng | `Accepted`, rồi mỗi lần khớp có `Trade` + sự kiện của maker, rồi `PartiallyFilled(taker, left)` nếu nó đã giao dịch, cuối cùng là `Expired(left)` |
| Hủy lệnh thành công | `Cancelled` |
| Hủy một lệnh không nằm chờ | `CancelRejected(NotResting)` (và `bool` trả về là `false`) |
| Giảm ít hơn số lượng còn lại | `Reduced` |
| Giảm bằng hoặc nhiều hơn số lượng còn lại | `Cancelled` |
| Giảm 0 | `CancelRejected(ZeroQuantity)` |
| Taker gặp lệnh của chính tài khoản nó, chế độ `CancelResting` | ..., `SelfTradePrevented`, `Cancelled(maker)`, rồi việc khớp lệnh tiếp tục |
| Taker gặp lệnh của chính tài khoản nó, chế độ `CancelIncoming` | ..., `SelfTradePrevented`, `PartiallyFilled(taker, left)` nếu nó đã giao dịch, `Expired(left)` |
| Hủy hàng loạt | `Cancelled` cho mỗi lệnh, theo thứ tự trong sổ, rồi `MassCancelled(count)` |

## 7.3 Trait `EventSink`

```rust
pub trait EventSink {
    /// Accepts the next event.
    fn emit(&mut self, event: Event);
}
```

Chỉ có một phương thức: "đây là sự kiện tiếp theo". Crate cài đặt trait này cho
ba loại sink, và bạn cũng có thể tự cài đặt cho kiểu của riêng mình.

| Sink | Có cấp phát không? | Giữ lại | Dùng khi |
|---|---|---|---|
| `Vec<Event>` | chỉ khi tăng dung lượng; tái sử dụng nó (bằng `clear`) thì sau giai đoạn khởi động nó sẽ không cấp phát nữa | mọi sự kiện | kiểm thử, bên gọi đơn giản, bất cứ chỗ nào cần danh sách đầy đủ |
| closure `FnMut(Event)` | chỉ khi bản thân closure cấp phát | không giữ lại sự kiện | chuyển tiếp thẳng từng sự kiện đi nơi khác: vào hàng đợi, bộ đếm, nhật ký |
| `EventBatch` | **không bao giờ** | 64 sự kiện đầu tiên của một lần gọi, và đếm số còn lại | bên gọi tuyệt đối không được cấp phát, và có kiểm tra xem sự kiện có bị cắt bớt không |

Dưới đây là cả ba cách chạy thực tế. Ví dụ y hệt có trong rustdoc của
[`src/events.rs`](../../src/events.rs), nơi `cargo test` biên dịch và chạy nó:

```rust
use matching_engine::{Engine, Event, EventBatch, OrderType, Side};

let mut engine = Engine::new(16);

// 1. A `Vec<Event>` keeps everything. Reuse it (with `clear`) and it stops
//    allocating once it has grown to fit the largest call.
let mut events = Vec::new();
engine.process_order(1, Side::Sell, 100, 5, &mut events);
assert_eq!(
    events,
    [Event::OrderAccepted {
        order_id: 1,
        account: 0,
        side: Side::Sell,
        order_type: OrderType::Limit,
        price_tick: 100,
        qty: 5,
    }]
);

// 2. A closure sees each event as it happens; this one counts trades.
let mut trades = 0;
engine.process_order(2, Side::Buy, 100, 2, &mut |event: Event| {
    if matches!(event, Event::Trade { .. }) {
        trades += 1;
    }
});
assert_eq!(trades, 1);

// 3. An `EventBatch` never allocates. It keeps the first 64 events of a
//    call and only counts the rest, so check `is_truncated` afterwards.
let mut batch = EventBatch::new();
engine.process_order(3, Side::Buy, 100, 3, &mut batch);
assert!(!batch.is_truncated());
assert_eq!(batch.as_slice().last(), Some(&Event::OrderFilled { order_id: 3 }));
```

### Tại sao dùng sink thay vì giá trị trả về?

Một phiên bản đầu của bộ khớp lệnh trả về một `EventBatch` cố định 64 ô sau mỗi lần
gọi. Thiết kế đó có hai hạn chế, và cả hai đều là bài học hữu ích:

1. **Mất sự kiện mà không báo.** Một taker quét qua 32 maker trở lên sẽ tạo ra
   hơn 64 sự kiện. Sổ lệnh vẫn được cập nhật đúng, nhưng các sự kiện của những
   giao dịch còn lại bị mất mà không báo; vì vậy, bên đối chiếu vị thế từ sự kiện
   sẽ bỏ sót một số lần khớp.
2. **Chi phí ngầm.** "Không có `malloc`" không có nghĩa là "không tốn gì": mỗi lần
   gọi đều khởi tạo và sao chép một mảng 2 KiB, kể cả với lệnh chỉ tạo ra một sự
   kiện.

Với sink, bộ khớp lệnh không cần biết trước một lần gọi sẽ tạo ra bao nhiêu sự kiện. Và
vì `process_order` là generic theo kiểu của sink (`S: EventSink + ?Sized`), trình
biên dịch tạo ra một bản chuyên biệt cho từng loại sink (đơn hình hóa,
monomorphization) và inline `emit`: một sink dạng closure không tốn hơn đoạn mã
viết tay.

## 7.4 Chuyện gì xảy ra khi có quá nhiều sự kiện?

Bộ khớp lệnh không bao giờ bỏ sót sự kiện, nhưng một sink **có giới hạn** thì có thể
hết chỗ. `EventBatch` được thiết kế cho trường hợp này: một mảng cố định, không cấp phát,
và báo rõ khi không đủ chỗ lưu sự kiện.

```rust
pub const MAX_EVENTS_PER_CALL: usize = 64;

pub struct EventBatch {
    events: [Event; MAX_EVENTS_PER_CALL],   // fixed array, no heap
    len: usize,                             // how many are stored
    dropped: usize,                         // how many did not fit since the last clear()
}
```

```text
   events:  [ e0 | e1 | e2 | e3 | ... các ô chưa dùng (giá trị cũ hoặc mặc định) ... ]     len = 4
              ^^^^^^^^^^^^^^^^^^^
              as_slice() chỉ trả về những ô này
```

- `push` (được gọi qua `emit`) ghi vào `events[len]` khi còn chỗ; nếu hết chỗ,
  nó chỉ tăng `dropped`. Phương thức này không bao giờ panic hay cấp phát.
- `dropped()` cho biết có bao nhiêu sự kiện không lưu được, và `is_truncated()` chính
  là `dropped() > 0`. **Hãy kiểm tra nó sau mỗi lần gọi** khi bạn dùng batch.
- `clear()` đặt `len` và `dropped` về 0 mà không xóa sạch mảng, nên tái sử dụng
  batch ít tốn chi phí: tạo sẵn một batch từ đầu và gọi `clear()` trước mỗi lần gọi bộ khớp lệnh.
- `as_slice()`, `len()`, `is_empty()` và `IntoIterator for &EventBatch` giúp đọc
  dữ liệu một cách tiện lợi.

**Kích thước** (đã đo): `64 × 40 + 8 + 8 = 2,576` byte.

Vậy `EventBatch` lưu đầy đủ sự kiện cho tối đa bao nhiêu maker? Mỗi maker được khớp tạo
ra hai sự kiện (`Trade` cộng với `OrderFilled` hoặc `OrderPartiallyFilled` của
maker), cộng thêm một
`OrderAccepted` ở đầu và một sự kiện cuối của taker:

```text
   events = 1 + 2 × (số maker bị chạm tới) + 1  <=  64
                                                 =>  tối đa 31 maker được báo cáo đầy đủ
```

| Số maker bị một taker quét qua | Số sự kiện tạo ra | `EventBatch` giữ lại | `dropped()` |
|---|---|---|---|
| 30 | 62 | 62 | 0 |
| 31 | 64 | 64 | 0 |
| 32 | 66 | 64 | 2 (`OrderFilled` của maker cuối cùng và `OrderFilled` của taker) |
| 40 | 82 | 64 | 18 |

Dòng 40 maker chính là kiểm thử đơn vị `bounded_batch_sink_reports_truncation`. Kiểm thử
nằm cạnh nó, `deep_sweep_reports_every_execution`, chạy cùng chuỗi khớp lệnh đó với một
`Vec<Event>` và kiểm tra rằng cả 40 giao dịch lẫn 40 lần khớp của maker đều được
ghi nhận, với `OrderFilled` của taker đứng cuối.

Khi một batch báo có sự kiện bị bỏ qua, sổ lệnh vẫn hoàn toàn đúng; chỉ có báo cáo là
không đầy đủ. Bạn có các lựa chọn sau:

1. Dùng sink không giới hạn (`Vec`, hoặc closure chuyển tiếp từng sự kiện đi
   tiếp).
2. Coi `is_truncated()` là một lỗi trong mã của bạn, chẳng hạn bằng cách dựng
   lại bản sao sổ lệnh của bạn từ `depth_snapshot`.
3. Giới hạn một lệnh được quét qua bao nhiêu mức, ngay từ trước khi nó tới
   bộ khớp lệnh.

## 7.5 Khi sink bị panic

Sink là nơi duy nhất bộ khớp lệnh gọi mã mà nó không kiểm soát. Vì vậy, bộ khớp lệnh tuân
theo một quy tắc đơn giản: **chỉ gọi `emit` giữa những lần thay đổi trạng thái
đã hoàn tất.** Đó là lý do trong vòng lặp khớp lệnh, mức giá vừa cạn được xóa
khỏi bitset *trước khi* báo lần khớp cuối cùng của maker
([mục 6.3](06-matching-algorithm.md)). Mọi cấu trúc dữ liệu đều nhất quán mỗi
khi sink chạy.

Dù vậy, sink vẫn không nên panic. Nếu nó panic:

- Nếu chương trình được biên dịch với `panic = "abort"`, tiến trình sẽ dừng hẳn. Có
  thể khôi phục bằng cách phát lại (replay) write-ahead log, như sau các sự cố
  dừng đột ngột (chương 9). Cấu hình release của crate này có đặt tùy chọn đó, nhưng nó chỉ áp
  dụng cho các tệp thực thi được biên dịch từ chính package này (kiểm thử, ví dụ và benchmark
  của package). Cargo bỏ qua cấu hình biên dịch của các dependency, nên nếu bạn dùng bộ khớp lệnh như một thư
  viện thì `Cargo.toml` của *bạn* mới là nơi quyết định; xem
  [mục 10.5](10-testing-and-performance.md#105-các-cấu-hình-biên-dịch-build-profile).
- Nếu panic bị bắt lại (bằng `catch_unwind`), sổ lệnh vẫn nhất quán, nhưng yêu cầu
  bị gián đoạn mới chỉ được áp dụng **một phần**: một số giao dịch đã xảy ra,
  còn phần dư của taker thì chưa bao giờ nằm chờ trong sổ. Lúc này bộ khớp lệnh khác với
  kết quả mà việc phát lại các yêu cầu đã ghi nhật ký sẽ tạo ra, nên **hãy loại bỏ trạng thái hiện tại
  và khôi phục từ nhật ký**.

Kiểu của Rust thể hiện rõ tình huống này: `&mut Engine` không phải `UnwindSafe`,
nên `catch_unwind` từ chối closure cho đến khi bạn bọc nó trong
`AssertUnwindSafe`. Lớp bọc này thể hiện rằng bạn chấp nhận khả năng trạng thái đã
được cập nhật một phần. Kiểm thử đơn vị `panicking_sink_leaves_the_book_consistent`
làm đúng việc này.

## 7.6 Nhận và xử lý sự kiện

Một hàm thông thường cũng dùng được làm sink, vì nó cài đặt `FnMut(Event)`:

```rust
use matching_engine::{Engine, Event, NewOrder, OrderType, Side};

fn log_event(event: Event) {
    match event {
        Event::OrderAccepted { order_id, account, side, order_type, price_tick, qty } => {
            println!("accepted  #{order_id} for {account}: {order_type:?} {side:?} {qty} @ {price_tick}")
        }
        Event::Trade { trade_id, maker_order_id, taker_order_id, taker_side, price_tick, qty } => {
            println!(
                "trade {trade_id}   {qty} @ {price_tick}, {taker_side:?} aggressor \
                 (maker #{maker_order_id}, taker #{taker_order_id})"
            )
        }
        Event::OrderPartiallyFilled { order_id, remaining_qty } => {
            println!("partial   #{order_id}, {remaining_qty} left")
        }
        Event::OrderFilled { order_id } => println!("filled    #{order_id}"),
        Event::OrderExpired { order_id, expired_qty } => {
            println!("expired   #{order_id}, {expired_qty} never rested")
        }
        Event::OrderCancelled { order_id, cancelled_qty, .. } => {
            println!("cancelled #{order_id}, {cancelled_qty} removed")
        }
        Event::OrderReduced { order_id, reduced_qty, remaining_qty, .. } => {
            println!("reduced   #{order_id} by {reduced_qty}, {remaining_qty} left")
        }
        Event::OrderRejected { order_id, reason } => println!("rejected  #{order_id}: {reason:?}"),
        Event::SelfTradePrevented { taker_order_id, maker_order_id } => {
            println!("prevented #{taker_order_id} trading with its own #{maker_order_id}")
        }
        Event::MassCancelled { account, cancelled_orders } => {
            println!("mass cancel of {account:?}: {cancelled_orders} orders")
        }
        Event::CancelRejected { order_id, reason } => {
            println!("cancel or reduce of #{order_id} rejected: {reason:?}")
        }
    }
}

let mut engine = Engine::new(16);
engine.process_order(1, Side::Sell, 100, 10, &mut log_event);
let ioc = NewOrder::limit(2, Side::Buy, 100, 14).with_type(OrderType::ImmediateOrCancel);
engine.submit_order(ioc, &mut log_event);
```

Kiểm thử thông qua sự kiện cũng đơn giản; các kiểm thử của chính bộ khớp lệnh được viết
theo cách này (chúng bọc hai dòng gọi bộ khớp lệnh trong một hàm trợ giúp nhỏ tên
`place`):

```rust
let mut engine = Engine::new(16);
let mut events = Vec::new();
engine.process_order(1, Side::Sell, 100, 10, &mut events);
events.clear();
engine.process_order(2, Side::Buy, 100, 4, &mut events);

assert_eq!(
    events,
    [
        Event::OrderAccepted {
            order_id: 2,
            account: 0,
            side: Side::Buy,
            order_type: OrderType::Limit,
            price_tick: 100,
            qty: 4,
        },
        Event::Trade {
            trade_id: 1,
            maker_order_id: 1,
            taker_order_id: 2,
            taker_side: Side::Buy,
            price_tick: 100,
            qty: 4,
        },
        Event::OrderPartiallyFilled { order_id: 1, remaining_qty: 6 },
        Event::OrderFilled { order_id: 2 },
    ]
);
```

## 7.7 Tự thực hành

Chạy các kiểm thử về sự kiện và hai kiểm thử của bộ khớp lệnh có so sánh các loại sink:

```sh
cargo test events::
cargo test deep_sweep_reports_every_execution
cargo test bounded_batch_sink_reports_truncation
```

Tiếp theo, hãy thử một closure sink. Trong
[`examples/walkthrough.rs`](../../examples/walkthrough.rs), thay `Vec<Event>` trong
`place` bằng một closure chỉ in các sự kiện `Trade` ngay khi chúng xảy ra, rồi
kiểm tra rằng kết quả in ra vẫn khớp với mục 1.10.

**Tiếp theo:** [8. Bộ đệm vòng (ring buffer)](08-ring-buffer.md)
