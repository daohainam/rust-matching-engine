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
    OrderAccepted        { order_id: u64 },
    Trade                { maker_order_id: u64, taker_order_id: u64, price_tick: u32, qty: u64 },
    OrderPartiallyFilled { order_id: u64, remaining_qty: u64 },
    OrderFilled          { order_id: u64 },
    OrderCancelled       { order_id: u64 },
    OrderRejected        { order_id: u64, reason: RejectReason },
}

pub enum RejectReason {
    // Input errors: the order is rejected alone, before OrderAccepted.
    ZeroQuantity,          // qty was 0
    ReservedOrderId,       // order_id was u64::MAX
    PriceOutOfRange,       // price_tick was 4096 or more
    DuplicateOrderId,      // a resting order already has this id
    // The remainder could not rest: always after OrderAccepted.
    BookFull,              // every arena slot is in use
    LevelQuantityOverflow, // the level's total would not fit in a u64
}
```

| Sự kiện | Được phát ra khi | Nói về ai |
|---|---|---|
| `OrderAccepted` | Một lệnh mới đã qua bước kiểm tra hợp lệ. | lệnh vừa đến |
| `Trade` | Hai lệnh khớp với nhau. | cả maker **và** taker, ở giá của maker |
| `OrderPartiallyFilled` | Một lệnh đã giao dịch nhưng vẫn còn số lượng. | một maker (sau mỗi lần khớp một phần), hoặc taker (một lần, ở cuối) |
| `OrderFilled` | Một lệnh không còn gì nữa. | một maker (khi nó bị khớp hết), hoặc taker |
| `OrderCancelled` | Một lệnh đang chờ bị hủy theo yêu cầu. | lệnh bị hủy |
| `OrderRejected` | Một lệnh bị từ chối: đầu vào sai, hoặc không còn chỗ để nằm chờ. `reason` cho biết là lý do nào. | lệnh vừa đến |

`Event` derive `Debug, Clone, Copy, PartialEq, Eq`, nên sao chép sự kiện rất rẻ
và so sánh chúng trong test rất dễ. Một `Event` nặng 32 byte (biến thể lớn nhất là
`Trade`, chứa `8 + 8 + 4 + 8 = 28` byte cộng với tag và padding). Thêm
`RejectReason` vào `OrderRejected` không tốn thêm gì: `8 + 1` byte vẫn nằm vừa
trong chỗ mà `Trade` vốn đã cần.

## 7.2 Quy tắc về thứ tự

Với mỗi lần gọi `process_order`, các sự kiện luôn có dạng sau:

```text
   [OrderAccepted]                                   <- luôn đứng đầu (trừ khi bị từ chối ngay từ đầu)
   ( Trade, <maker Filled | maker PartiallyFilled> ) <- lặp lại mỗi lần khớp, theo đúng thứ tự khớp
   [ taker OrderFilled | taker OrderPartiallyFilled ]<- nhiều nhất một sự kiện, cho biết trạng thái cuối của taker
   [ OrderRejected ]                                 <- chỉ khi lệnh không nằm chờ được (và chưa giao dịch lần nào)
```

Những điểm cần nhớ:

- Một `Trade` **luôn được theo ngay sau** bởi sự kiện mô tả trạng thái mới của
  *maker*.
- `OrderPartiallyFilled` hoặc `OrderFilled` của **taker** chỉ xuất hiện **một
  lần**, ở cuối, chứ không xuất hiện sau mỗi giao dịch. Muốn theo dõi tiến độ của
  một taker, hãy cộng `qty` của các sự kiện `Trade` của nó lại.
- Một taker không giao dịch lần nào mà chỉ nằm chờ thì chỉ tạo ra **duy nhất**
  `OrderAccepted`. Không có sự kiện riêng kiểu "lệnh đang nằm chờ".
- Lệnh bị từ chối vì đầu vào sai thì sự kiện đến **một mình**; lệnh bị từ chối vì
  hết dung lượng thì sự kiện đến **sau** `OrderAccepted`, và chỉ với lệnh chưa
  giao dịch lần nào (xem
  [mục 6.4](06-matching-algorithm.md#64-giai-đoạn-4-cho-phần-còn-lại-nằm-chờ-trong-sổ)).
- Bản thân engine **không bao giờ làm rơi sự kiện**. Chỉ có sink có giới hạn, như
  `EventBatch`, mới có thể hết chỗ (mục 7.4).

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
| Lệnh không nằm chờ được (arena đầy / tràn số) | `Accepted`, `Rejected(BookFull / LevelQuantityOverflow)` (chưa có giao dịch nào) |
| Hủy lệnh thành công | `Cancelled` |
| Hủy một id không tồn tại | *(không có sự kiện nào; `bool` trả về là `false`)* |

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
use matching_engine::{Engine, Event, EventBatch, Side};

let mut engine = Engine::new(16);

// 1. A `Vec<Event>` keeps everything. Reuse it (with `clear`) and it stops
//    allocating once it has grown to fit the largest call.
let mut events = Vec::new();
engine.process_order(1, Side::Sell, 100, 5, &mut events);
assert_eq!(events, [Event::OrderAccepted { order_id: 1 }]);

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

**Kích thước** (đã đo): `64 × 32 + 8 + 8 = 2,064` byte.

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
use matching_engine::{Engine, Event, Side};

fn log_event(event: Event) {
    match event {
        Event::OrderAccepted { order_id } => println!("accepted  #{order_id}"),
        Event::Trade { maker_order_id, taker_order_id, price_tick, qty } => println!(
            "trade     {qty} @ {price_tick} (maker #{maker_order_id}, taker #{taker_order_id})"
        ),
        Event::OrderPartiallyFilled { order_id, remaining_qty } => {
            println!("partial   #{order_id}, {remaining_qty} left")
        }
        Event::OrderFilled { order_id } => println!("filled    #{order_id}"),
        Event::OrderCancelled { order_id } => println!("cancelled #{order_id}"),
        Event::OrderRejected { order_id, reason } => println!("rejected  #{order_id}: {reason:?}"),
    }
}

let mut engine = Engine::new(16);
engine.process_order(1, Side::Sell, 100, 10, &mut log_event);
engine.process_order(2, Side::Buy, 100, 4, &mut log_event);
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
        Event::OrderAccepted { order_id: 2 },
        Event::Trade { maker_order_id: 1, taker_order_id: 2, price_tick: 100, qty: 4 },
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
