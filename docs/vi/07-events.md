# 7. Sự kiện (events): engine báo cáo những gì đã xảy ra

> *Bản gốc tiếng Anh: [07-events.md](../07-events.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Mã nguồn: [`src/events.rs`](../../src/events.rs)

Engine không in ra gì cả, không ghi vào socket, và cũng không quyết định kết quả
của nó đi đâu. Mỗi lần gọi `process_order` hoặc `cancel_order` đều nhận thêm một
tham số, gọi là **event sink** (nơi nhận sự kiện), và engine đưa từng sự kiện cho
sink ngay khoảnh khắc nó xảy ra. Bên gọi tự quyết định làm gì với chúng (phát dữ
liệu thị trường, cập nhật tài khoản, ghi log...).

Nhờ vậy engine là một máy trạng thái (state machine) thuần túy và tất định
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

| Sự kiện | Được phát ra khi | Nói về ai |
|---|---|---|
| `OrderAccepted` | Một lệnh mới đã qua bước kiểm tra hợp lệ. Sự kiện nhắc lại tài khoản, phía, loại lệnh, giá giới hạn và số lượng của lệnh (với lệnh thị trường, đó là giá giới hạn mà nó được gán: mép của biên độ giá (price band), hoặc mức giá tệ nhất mà phía của nó cho phép). | lệnh vừa đến |
| `Trade` | Hai lệnh khớp với nhau. `taker_side` là phía của bên chủ động (aggressor); `trade_id` đánh số các giao dịch 1, 2, 3, ... | cả maker **và** taker, ở giá của maker |
| `OrderPartiallyFilled` | Một lệnh đã giao dịch nhưng vẫn còn số lượng. | một maker (sau mỗi lần khớp một phần), hoặc taker (một lần, ở cuối) |
| `OrderFilled` | Một lệnh không còn gì nữa. | một maker (khi nó bị khớp hết), hoặc taker |
| `OrderExpired` | Phần còn lại của một lệnh immediate-or-cancel hoặc lệnh thị trường bị bỏ đi; nó chưa bao giờ vào sổ. | taker (một lần, ở cuối cùng) |
| `OrderCancelled` | Một lệnh đang chờ bị hủy theo yêu cầu (hoặc bị giảm đi toàn bộ số lượng còn lại). Sự kiện cho biết lệnh đã nằm chờ ở đâu và bao nhiêu số lượng đã rời khỏi sổ. | lệnh bị hủy |
| `OrderReduced` | Số lượng của một lệnh đang chờ được giảm theo yêu cầu; lệnh vẫn giữ nguyên vị trí trong hàng đợi. | lệnh bị giảm |
| `OrderRejected` | Một lệnh bị từ chối: đầu vào sai, vi phạm một giới hạn rủi ro, một lệnh post-only sẽ gây ra giao dịch, hoặc không còn chỗ để nằm chờ. `reason` cho biết là lý do nào. | lệnh vừa đến |
| `SelfTradePrevented` | Một lệnh vừa đến gặp một lệnh đang chờ của chính tài khoản đó, và chế độ chống tự giao dịch (self-trade prevention) của nó đã chặn giao dịch. Theo sau là `OrderCancelled` của maker, `OrderExpired` của taker, hoặc cả hai, tùy theo chế độ. | cả taker **và** maker |
| `MassCancelled` | Một lần gọi `cancel_account` (`account: Some(..)`) hoặc `cancel_all` (`None`) đã xong; `cancelled_orders` đếm số sự kiện `OrderCancelled` đứng trước nó. | yêu cầu đó |
| `CancelRejected` | Một yêu cầu hủy hoặc giảm lệnh bị từ chối: không có lệnh đang chờ nào mang id đó, hoặc lượng giảm bằng 0. `reason` cho biết là lý do nào. | id trong yêu cầu |

`Event` derive `Debug, Clone, Copy, PartialEq, Eq`, nên sao chép sự kiện rất rẻ
và so sánh chúng trong test rất dễ. Một `Event` nặng 40 byte (biến thể lớn nhất là
`Trade`, chứa `8 + 8 + 8 + 8 + 4 + 1 = 37` byte, cộng với tag, rồi làm tròn lên bội
số của 8). Các biến thể khác đều nằm vừa trong chỗ mà `Trade` vốn đã cần.

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

- Một `Trade` **luôn được theo ngay sau** bởi sự kiện mô tả trạng thái mới của
  *maker*.
- Khi có chống tự giao dịch, một `SelfTradePrevented` thế chỗ cho một `Trade`
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
  sẽ gây ra giao dịch) thì sự kiện đến **một mình**; lệnh bị từ chối vì hết dung
  lượng thì sự kiện đến **sau** `OrderAccepted`, và chỉ với lệnh chưa
  giao dịch lần nào (xem
  [mục 6.4](06-matching-algorithm.md#64-giai-đoạn-4-cho-phần-còn-lại-nằm-chờ-trong-sổ)).
- Bản thân engine **không bao giờ làm rơi sự kiện**. Chỉ có sink có giới hạn, như
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

Các sự kiện mang đủ thông tin để giữ một bản sao độ sâu của sổ lệnh mà không cần
nhìn thấy các yêu cầu. Đó chính là việc mà một hệ thống phát dữ liệu thị trường
hay một hệ thống quản lý rủi ro làm:

| Sự kiện | Thay đổi trên bản sao |
|---|---|
| `OrderAccepted { side, price_tick, qty, .. }` | ghi nhớ các thông số này cho đến khi lần gọi kết thúc |
| `Trade { taker_side, price_tick, qty, .. }` | mức giá của maker mất đi `qty`; mức đó nằm ở phía **đối diện** với `taker_side`, tại `price_tick` |
| `OrderCancelled { side, price_tick, cancelled_qty, .. }` | mức giá đó mất đi `cancelled_qty` |
| `OrderReduced { side, price_tick, reduced_qty, .. }` | mức giá đó mất đi `reduced_qty` |
| `SelfTradePrevented`, `MassCancelled` | không có gì: các sự kiện `OrderCancelled` quanh chúng mang theo các thay đổi |
| kết thúc một lần gọi `submit_order` | trừ khi sự kiện cuối cùng của taker là `OrderFilled`, `OrderExpired` hoặc `OrderRejected`, phần còn lại của nó (`qty` trừ đi các giao dịch của nó) giờ nằm chờ tại chính `price_tick` của nó |

`trade_id` giúp xử lý các chỗ hổng và bản trùng lặp nói ở
[mục 9.5](09-write-ahead-log.md#95-đọc-và-phát-lại): engine đánh số các giao dịch
theo đúng thứ tự chúng xảy ra, và engine là tất định, nên phát lại cùng các yêu cầu
đó sẽ gán ra đúng những id y hệt. Một bên tiêu thụ đã thấy giao dịch 41 có thể bỏ
qua mọi giao dịch có id nhỏ hơn, còn một bước nhảy từ 41 lên 43 nghĩa là nó đã bỏ
lỡ một giao dịch.

### Các chuỗi sự kiện mẫu

Các chuỗi này lấy thẳng từ unit test trong `engine.rs`.

| Tình huống | Sự kiện |
|---|---|
| Lệnh nằm chờ, không có gì khớp | `Accepted` |
| Khớp vừa khít, taker bằng maker | `Accepted`, `Trade`, `Filled(maker)`, `Filled(taker)` |
| Taker nhỏ hơn maker | `Accepted`, `Trade`, `PartiallyFilled(maker, left)`, `Filled(taker)` |
| Taker lớn hơn maker; phần còn lại nằm chờ | `Accepted`, `Trade`, `Filled(maker)`, `PartiallyFilled(taker, left)` |
| Taker quét qua nhiều mức giá | `Accepted`, rồi mỗi lần khớp có `Trade` + sự kiện của maker, cuối cùng là sự kiện cuối của taker |
| Đầu vào không hợp lệ / trùng id | `Rejected(ZeroQuantity / ReservedOrderId / PriceOutOfRange / DuplicateOrderId)` |
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
| `Vec<Event>` | chỉ khi nó đang lớn lên; tái sử dụng nó (bằng `clear`) thì sau giai đoạn khởi động nó sẽ không cấp phát nữa | mọi sự kiện | test, bên gọi đơn giản, bất cứ chỗ nào cần danh sách đầy đủ |
| closure `FnMut(Event)` | chỉ khi bản thân closure cấp phát | không giữ gì | chuyển tiếp thẳng từng sự kiện đi nơi khác: vào hàng đợi, bộ đếm, log |
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

Một phiên bản đầu của engine trả về một `EventBatch` cố định 64 ô sau mỗi lần
gọi. Thiết kế đó phải trả hai cái giá, và cả hai đều đáng để học:

1. **Mất sự kiện âm thầm.** Một taker quét qua 32 maker trở lên sẽ tạo ra hơn 64
   sự kiện. Sổ lệnh vẫn được cập nhật đúng, nhưng báo cáo cho các giao dịch dư ra
   thì biến mất lặng lẽ, nên ai đối chiếu vị thế từ các sự kiện sẽ bỏ sót những
   lần khớp.
2. **Chi phí ngầm.** "Không có `malloc`" không có nghĩa là "không tốn gì": mỗi lần
   gọi đều khởi tạo và sao chép một mảng 2 KiB, kể cả với lệnh chỉ tạo ra một sự
   kiện.

Với sink, engine không cần biết trước một lần gọi sẽ tạo ra bao nhiêu sự kiện. Và
vì `process_order` là generic theo kiểu của sink (`S: EventSink + ?Sized`), trình
biên dịch tạo ra một bản chuyên biệt cho từng loại sink (đơn hình hóa,
monomorphization) và inline `emit`: một sink dạng closure không tốn hơn đoạn code
viết tay.

## 7.4 Chuyện gì xảy ra khi có quá nhiều sự kiện?

Engine không bao giờ làm rơi sự kiện, nhưng một sink **có giới hạn** thì có thể
hết chỗ. `EventBatch` sinh ra cho trường hợp đó: một mảng cố định, không cấp phát,
và trung thực về chuyện tràn.

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

- `push` (được gọi qua `emit`) ghi vào `events[len]` khi còn chỗ; sau đó nó chỉ
  tăng `dropped`. Nó không bao giờ panic và không bao giờ cấp phát.
- `dropped()` cho biết có bao nhiêu sự kiện không vừa, và `is_truncated()` chính
  là `dropped() > 0`. **Hãy kiểm tra nó sau mỗi lần gọi** khi bạn dùng batch.
- `clear()` đặt `len` và `dropped` về 0 mà không xóa sạch mảng, nên tái sử dụng
  batch rất rẻ: tạo sẵn một cái từ đầu và gọi `clear()` trước mỗi lần gọi engine.
- `as_slice()`, `len()`, `is_empty()` và `IntoIterator for &EventBatch` giúp đọc
  dữ liệu một cách tiện lợi.

**Kích thước** (đã đo): `64 × 40 + 8 + 8 = 2,576` byte.

Vậy vừa được bao nhiêu maker? Mỗi maker bị chạm tới tốn hai sự kiện (`Trade` cộng
với `OrderFilled` hoặc `OrderPartiallyFilled` của maker), cộng thêm một
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

Dòng 40 maker chính là unit test `bounded_batch_sink_reports_truncation`. Test
nằm cạnh nó, `deep_sweep_reports_every_execution`, chạy cùng màn quét đó vào một
`Vec<Event>` và kiểm tra rằng cả 40 giao dịch lẫn 40 lần khớp của maker đều đến
nơi, với `OrderFilled` của taker đứng cuối.

Khi một batch báo là đã bị cắt bớt, sổ lệnh vẫn hoàn toàn đúng; chỉ có báo cáo là
không đầy đủ. Bạn có các lựa chọn sau:

1. Dùng sink không giới hạn (`Vec`, hoặc closure chuyển tiếp từng sự kiện đi
   tiếp).
2. Coi `is_truncated()` là một lỗi trong code của bạn, chẳng hạn bằng cách dựng
   lại bản sao sổ lệnh của bạn từ `depth_snapshot`.
3. Giới hạn một lệnh được quét qua bao nhiêu mức, ngay từ trước khi nó tới
   engine.

## 7.5 Khi sink bị panic

Sink là nơi duy nhất engine chạy code mà nó không kiểm soát được. Vì vậy engine
tuân theo một quy tắc đơn giản: **chỉ gọi `emit` giữa những lần thay đổi trạng
thái đã hoàn chỉnh.** Đó là lý do trong vòng lặp khớp lệnh, một mức giá đã cạn
được xóa khỏi bitset *trước khi* lần khớp của maker cuối cùng được báo cáo
([mục 6.3](06-matching-algorithm.md)). Mọi cấu trúc đều nhất quán với nhau bất cứ
khi nào sink của bạn chạy.

Dù vậy, sink vẫn không nên panic. Nếu nó panic:

- Nếu chương trình được build với `panic = "abort"`, tiến trình sẽ dừng hẳn. Bạn
  phục hồi bằng cách phát lại (replay) write-ahead log, như sau mọi vụ crash
  (chương 9). Release profile của crate này có đặt tùy chọn đó, nhưng nó chỉ áp
  dụng cho các binary build từ chính package này (test, example và bench của
  nó). Cargo bỏ qua profile của dependency, nên nếu bạn dùng engine như một thư
  viện thì `Cargo.toml` của *bạn* mới là nơi quyết định; xem
  [mục 10.5](10-testing-and-performance.md#105-các-build-profile).
- Nếu panic bị bắt lại (bằng `catch_unwind`), sổ lệnh vẫn nhất quán, nhưng yêu cầu
  bị gián đoạn mới chỉ được áp dụng **một phần**: một số giao dịch đã xảy ra,
  còn phần dư của taker thì chưa bao giờ nằm chờ trong sổ. Lúc này engine khác với
  kết quả mà việc phát lại các yêu cầu đã ghi log sẽ tạo ra, nên **hãy bỏ nó đi
  và dựng lại từ log**.

Rust cảnh báo bạn về đúng tình huống này: `&mut Engine` không phải `UnwindSafe`,
nên `catch_unwind` từ chối closure cho đến khi bạn bọc nó trong
`AssertUnwindSafe`. Đó chính là chữ ký của bạn dưới câu "tôi biết trạng thái có
thể đã được cập nhật dở". Unit test `panicking_sink_leaves_the_book_consistent`
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

Kiểm thử bằng sự kiện cũng trực tiếp không kém; các test của chính engine được viết
theo cách này (chúng bọc hai dòng gọi engine trong một hàm trợ giúp nhỏ tên
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

Chạy các test về sự kiện và hai test của engine có so sánh các loại sink:

```sh
cargo test events::
cargo test deep_sweep_reports_every_execution
cargo test bounded_batch_sink_reports_truncation
```

Sau đó hãy thử một closure sink. Trong
[`examples/walkthrough.rs`](../../examples/walkthrough.rs), thay `Vec<Event>` trong
`place` bằng một closure chỉ in các sự kiện `Trade` ngay khi chúng xảy ra, rồi
kiểm tra rằng kết quả in ra vẫn khớp với mục 1.10.

**Tiếp theo:** [8. Bộ đệm vòng (ring buffer)](08-ring-buffer.md)
