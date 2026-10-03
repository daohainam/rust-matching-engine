# 1. Kiến thức cơ bản về sổ lệnh

> *Bản gốc tiếng Anh: [01-order-book-basics.md](../01-order-book-basics.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

> **Chương này dành cho ai:** bất kỳ ai chưa từng nhìn vào bên trong một sàn giao
> dịch. Bạn không cần biết gì về tài chính. Nếu bạn đã biết "sổ lệnh giới hạn"
> (limit order book) là gì, hãy lướt qua ví dụ minh họa ở cuối chương rồi nhảy
> sang [chương 2](02-architecture.md).

## 1.1 Engine khớp lệnh là gì?

**Engine khớp lệnh** (matching engine) là phần của sàn giao dịch phải trả lời một
câu hỏi, hàng triệu lần mỗi ngày:

> *"Có người muốn mua, có người muốn bán. Ta ghép họ lại với nhau được không, và
> ở mức giá nào?"*

Mọi thứ khác trong repo này (arena, bitset, bảng băm...) sinh ra chỉ để trả lời
câu hỏi đó thật **nhanh** và thật **ổn định**.

## 1.2 Lệnh

**Lệnh** (order) là chỉ thị của một nhà giao dịch. Engine này chỉ hiểu một loại
lệnh: **lệnh giới hạn** (limit order). Một lệnh giới hạn gồm bốn thông tin:

| Trường     | Ý nghĩa                                                        | Trong code                  |
|------------|----------------------------------------------------------------|-----------------------------|
| `side`     | Nhà giao dịch muốn **mua** hay **bán**?                        | `Side::Buy` / `Side::Sell`  |
| `price`    | Mức giá *tệ nhất* mà nhà giao dịch chấp nhận.                  | `price_tick: u32`           |
| `qty`      | Số lượng đơn vị (cổ phiếu, lô, hợp đồng...).                   | `qty: u64`                  |
| `order_id` | Một nhãn duy nhất để gọi tên lệnh (ví dụ khi cần hủy).         | `order_id: u64`             |

"Duy nhất" ở đây nghĩa là *duy nhất trong số các lệnh còn nằm trong sổ*. Engine
từ chối lệnh mới nếu id của nó trùng với một lệnh đang chờ. Nhưng khi một lệnh đã
khớp hết hoặc đã bị hủy, engine quên id đó, nên id ấy có thể được dùng lại. Sàn
thật thường cấm việc này; ở đây, việc tránh trùng id là trách nhiệm của bên gọi
(caller).

Giá là một **giới hạn**, nghĩa là:

- Lệnh **mua** ở giá 100 nói rằng *"tôi trả tối đa 100"*.
- Lệnh **bán** ở giá 100 nói rằng *"tôi bán với giá tối thiểu 100"*.

## 1.3 Sổ lệnh

**Sổ lệnh** (order book) là danh sách các lệnh đang chờ một đối tác (counterparty)
để giao dịch. Sổ có hai phía:

- **Bid**: các lệnh mua đang chờ. Bid tốt nhất (*best bid*) là bid **cao nhất**:
  đó là mức giá có lợi nhất mà người bán có thể giao dịch ở thời điểm hiện tại.
- **Ask** (còn gọi là offer): các lệnh bán đang chờ. Ask tốt nhất (*best ask*) là
  ask **thấp nhất**: đó là mức giá rẻ nhất mà một người mua có thể mua được.

```text
            ASK  (người bán đang chờ)
   giá      SL
    103      8
    102      5
    101      7        <-- ask tốt nhất  (giá bán thấp nhất)
  ----------------------------------------------  spread
    100      6        <-- bid tốt nhất  (giá mua cao nhất)
     99     10
     98      4
            BID  (người mua đang chờ)
```

Khoảng cách giữa bid tốt nhất và ask tốt nhất gọi là **spread**. Trong một sổ lệnh
bình thường, bid tốt nhất luôn **thấp hơn** ask tốt nhất. Nếu hai giá này mà
chạm nhau thì một giao dịch xảy ra ngay lập tức. Đó chính là việc của engine khớp
lệnh.

> [!NOTE]
> Chữ **CLOB** trong phần mô tả dự án này là viết tắt của *Central Limit Order
> Book*: một sổ lệnh trung tâm duy nhất, chỉ gồm các lệnh giới hạn, mà mọi người
> đều giao dịch với nó.

## 1.4 Vượt spread: khi nào có giao dịch

Khi một lệnh mới đến, engine kiểm tra trước xem nó có giao dịch được ngay không.
Một lệnh mới **có thể khớp ngay** (marketable, hay còn nói là nó *vượt spread*)
khi:

- lệnh **mua** có giá **≥ ask tốt nhất**, hoặc
- lệnh **bán** có giá **≤ bid tốt nhất**.

```text
   ask tốt nhất = 101

   lệnh MUA mới @ 100  ->  100 < 101   -> chưa khớp ngay được, nằm chờ trong sổ
   lệnh MUA mới @ 101  ->  101 >= 101  -> khớp được, giao dịch với ask ở 101
   lệnh MUA mới @ 105  ->  105 >= 101  -> khớp được, giao dịch với ask ở 101
```

Dòng cuối chứa một điểm tinh tế mà người mới thường ngạc nhiên: người mua nói
"tối đa 105", nhưng giao dịch lại xảy ra ở giá **101**, tức giá của lệnh đã chờ
sẵn. **Giao dịch luôn diễn ra ở giá của lệnh đang chờ.** Người mua được giá tốt
hơn mức họ đưa ra (*price improvement*).

### Maker và taker

Mỗi giao dịch có hai lệnh tham gia:

- **Maker** là lệnh đã nằm sẵn trong sổ. Nó *tạo ra* thanh khoản (liquidity).
- **Taker** là lệnh vừa đến và *lấy đi* thanh khoản đó.

Bạn sẽ gặp hai từ này khắp nơi trong code (`maker_order_id`, `taker_order_id`,
`match_incoming`).

## 1.5 Ưu tiên giá, rồi đến thời gian

Nếu có nhiều lệnh đang chờ thì lệnh nào được phục vụ trước? Quy tắc mà hầu hết
các sàn (và engine này) dùng là **ưu tiên giá, rồi đến thời gian**
(price-time priority):

1. **Giá trước.** Giá tốt nhất được phục vụ trước. Với người mua, đó là ask thấp
   nhất; với người bán, đó là bid cao nhất.
2. **Rồi đến thời gian.** Trong các lệnh **cùng một giá**, lệnh đến **trước** được
   phục vụ trước (FIFO: vào trước, ra trước).

```text
   Mức giá 101 (ask):   [ #2 bán 3 ]  ->  [ #3 bán 4 ]
                          cũ nhất            mới nhất
                          khớp trước         khớp sau
```

Vì vậy, mỗi mức giá trong engine là một **hàng đợi** chứ không chỉ là một con số
(xem [chương 3](03-order-pool.md)).

## 1.6 Khớp một phần

Lệnh đến có thể lớn hơn số lượng đang có sẵn. Khi đó nó chỉ được khớp **một
phần**:

- Nếu taker lớn hơn maker, maker được khớp hết và rời khỏi sổ; taker đi tiếp sang
  lệnh đang chờ kế tiếp.
- Nếu taker nhỏ hơn maker, taker được khớp hết, còn maker ở lại trong sổ với số
  lượng còn lại nhỏ hơn.
- Nếu hai bên có đúng cùng số lượng, **cả hai** đều khớp hết cùng lúc: maker rời
  khỏi sổ và taker xong việc. (Engine phát một sự kiện `OrderFilled` cho từng lệnh.)
- Nếu sau khi đã giao dịch với mọi lệnh có thể mà taker vẫn còn dư số lượng, thì
  **phần dư nằm chờ** trong sổ như một lệnh đang chờ mới.

## 1.7 Hủy lệnh

Nhà giao dịch có thể rút một lệnh đang chờ bất cứ lúc nào bằng `order_id` của nó.
Engine phải tìm ra lệnh đó *mà không quét cả sổ*, gỡ nó ra khỏi giữa hàng đợi, và
nếu đó là lệnh cuối cùng ở mức giá ấy thì phải xóa luôn mức giá khỏi sổ. Làm được
việc này trong O(1) là một trong những thách thức thiết kế chính (xem
[chương 3 đến 5](03-order-pool.md)).

## 1.8 Tick: giá là những số nguyên nhỏ

Giá thật trông như `101.25`. Số dấu phẩy động không biểu diễn chính xác được
hầu hết các phân số thập phân và cho kết quả đáng ngạc nhiên (`0.1 + 0.2 != 0.3`),
điều không thể chấp nhận khi có tiền đổi chủ. Vì vậy các sàn dùng **tick là số
nguyên**. Một *tick* là bước giá nhỏ nhất được phép.

```text
   kích thước tick = 0.25    tick 0   -> giá 100.00   (nếu ta chọn mốc lệch (offset) là 100.00)
                             tick 1   -> giá 100.25
                             tick 2   -> giá 100.50
                             ...
                             tick 4095-> giá 1123.75
```

Engine này **chỉ làm việc với tick**: `price_tick` là một `u32` từ `0` đến `4095`
(gồm cả hai đầu), tức 4096 mức giá khác nhau. Việc đổi qua lại giữa giá thật và
tick (`price = offset + tick * tick_size`) là **trách nhiệm của bên gọi**.

Vì sao điều này quan trọng với thiết kế? Nếu tập các mức giá có thể có vừa nhỏ
vừa liền nhau, ta có thể dùng **mảng đánh chỉ số theo tick** thay cho cây hay bảng
băm. `levels[tick]` chỉ tốn một lần đọc bộ nhớ. Ý tưởng đó chống đỡ toàn bộ thiết
kế.

## 1.9 Engine này hỗ trợ gì (và không hỗ trợ gì)

| Có hỗ trợ                                              | Chưa hỗ trợ                                            |
|--------------------------------------------------------|--------------------------------------------------------|
| Lệnh giới hạn, mua và bán                              | Lệnh thị trường (market), lệnh dừng (stop)             |
| Khớp một phần và khớp đủ                               | Các biến thể thời hạn hiệu lực (IOC, FOK, lệnh trong ngày) |
| Cho phần chưa khớp nằm chờ trong sổ                    | Sửa / thay thế lệnh (hủy rồi đặt lệnh mới là được)     |
| Hủy theo `order_id`                                    | Chống tự giao dịch với chính mình, giới hạn theo tài khoản |
| Ưu tiên giá, rồi đến thời gian                         | Nhiều mã giao dịch trong một `Engine` (mỗi mã một engine) |
| Truy vấn độ sâu (`depth_at`, `depth_snapshot`)         | Phiên đấu giá, cầu dao ngắt thị trường, phí            |

### Lệnh sai thì sao?

Một số lệnh bị từ chối trước khi chạm được vào sổ. Engine phát sự kiện
`OrderRejected` nói rõ lý do, và không có gì khác thay đổi:

| Lệnh có...                                                                        | Lý do được báo       |
|-----------------------------------------------------------------------------------|----------------------|
| số lượng bằng 0                                                                   | `ZeroQuantity`       |
| id là `u64::MAX` (dành riêng cho nội bộ, xem [chương 5](05-hash-map.md))          | `ReservedOrderId`    |
| tick từ 4096 trở lên                                                              | `PriceOutOfRange`    |
| cùng id với một lệnh vẫn đang chờ                                                 | `DuplicateOrderId`   |

Một lệnh cũng có thể được chấp nhận và giao dịch bình thường, nhưng rồi **không
còn chỗ trong sổ** để phần dư nằm chờ. Lúc đó nó bị từ chối với lý do
`BookFull` (chương 6 giải thích khi nào điều này xảy ra).

Giới hạn phạm vi tính năng là có chủ ý: nhờ vậy code đủ ngắn để đọc trong một lần ngồi.
[Chương 11](11-limitations-and-exercises.md) liệt kê các ý tưởng mở rộng.

## 1.10 Một ví dụ hoàn chỉnh

Hãy theo dõi ví dụ này thật kỹ; ta sẽ dùng lại nó ở
[chương 6](06-matching-algorithm.md) để cho thấy bên trong engine làm gì. Mọi kết
quả bên dưới đều do chạy engine thật mà ra, và bạn có thể tự chạy lại:

```sh
cargo run --example walkthrough
```

Chương trình ([`examples/walkthrough.rs`](../../examples/walkthrough.rs)) gửi lần
lượt các lệnh dưới đây, rồi in ra các sự kiện và trạng thái sổ sau mỗi bước. Sổ
mà chương trình in ra cho thấy tổng số lượng ở mỗi mức giá; còn hàng đợi của từng
lệnh trong các sơ đồ bên dưới là thứ mà chương 3 và 6 sẽ xây dựng.

**Bước 1 đến 5: năm lệnh lần lượt đến, không lệnh nào khớp được, nên tất cả đều
nằm chờ.**

| # | Lệnh đến               | Có thể khớp ngay? | Kết quả      |
|---|------------------------|-------------|--------------|
| 1 | `#1` BÁN 5 @ 102       | chưa có bid | nằm chờ      |
| 2 | `#2` BÁN 3 @ 101       | chưa có bid | nằm chờ      |
| 3 | `#3` BÁN 4 @ 101       | chưa có bid | nằm chờ, **đứng sau** `#2` |
| 4 | `#4` MUA 10 @ 99       | 99 < ask tốt nhất 101 | nằm chờ  |
| 5 | `#5` MUA 6 @ 100       | 100 < ask tốt nhất 101 | nằm chờ |

Lúc này sổ lệnh trông như sau:

```text
            ASK
   tick   tổng   hàng đợi (cũ nhất -> mới nhất)
    102      5    [#1:5]
    101      7    [#2:3] -> [#3:4]        <-- ask tốt nhất = 101
  ---------------------------------------------------------
    100      6    [#5:6]                  <-- bid tốt nhất = 100
     99     10    [#4:10]
            BID
```

**Bước 6: lệnh `#6` MUA 14 @ 102 đến.** Nó có thể khớp ngay (102 ≥ 101). Engine đi
lên phía ask, từ giá rẻ nhất trở đi:

| Lần khớp | Với          | Giá   | SL  | Vì sao                                               |
|----------|--------------|-------|-----|------------------------------------------------------|
| 1        | `#2` (maker) | 101   | 3   | cũ nhất ở giá tốt nhất; `#2` khớp hết                |
| 2        | `#3` (maker) | 101   | 4   | lệnh kế tiếp trong hàng đợi FIFO; `#3` khớp hết      |
| 3        | `#1` (maker) | 102   | 5   | mức 101 đã hết lệnh, chuyển lên 102; `#1` khớp hết   |

Tổng đã khớp: 3 + 4 + 5 = 12. Lệnh yêu cầu 14, nên **còn dư 2 đơn vị**. Không còn
ask nào nữa, vì vậy phần dư **nằm chờ như một bid ở giá 102**:

```text
            ASK
   (trống)
  ---------------------------------------------------------
    102      2    [#6:2]                  <-- bid tốt nhất = 102   (phần dư)
    100      6    [#5:6]
     99     10    [#4:10]
            BID
```

Sau đó `#4` bị hủy: mức bid 99 biến mất, chỉ còn các bid ở 102 và 100.

Chuỗi **sự kiện** chính xác mà engine phát ra cho bước 6 là:

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

Sự kiện là cách engine nói chuyện với thế giới bên ngoài; xem
[chương 7](07-events.md).

## 1.11 Thuật ngữ

| Thuật ngữ       | Ý nghĩa                                                                  |
|-----------------|--------------------------------------------------------------------------|
| Bid             | Một lệnh mua đang chờ                                                    |
| Ask / Offer     | Một lệnh bán đang chờ                                                    |
| Best bid / ask  | Bid cao nhất / ask thấp nhất, là các mức giá hấp dẫn nhất ở mỗi phía     |
| Spread          | Ask tốt nhất trừ bid tốt nhất                                            |
| Lệnh giới hạn   | "Giao dịch ở mức giá này hoặc tốt hơn"                                   |
| Marketable      | Có thể giao dịch ngay với phía đối diện                                  |
| Maker           | Lệnh đang chờ trong một giao dịch                                        |
| Taker           | Lệnh vừa đến trong một giao dịch                                         |
| Mức giá (price level) | Tất cả các lệnh đang chờ ở cùng một giá                            |
| Độ sâu (depth)  | Tổng số lượng có sẵn ở một mức giá (hoặc qua nhiều mức)                  |
| Fill            | Một lần khớp; *khớp một phần* nghĩa là vẫn còn số lượng dư               |
| Tick            | Bước giá nhỏ nhất; engine này dùng tick nguyên từ 0 đến 4095             |
| Ưu tiên giá-thời gian | Giá tốt hơn trước, rồi đến lệnh đến sớm hơn                        |
| Arena           | Một mảng lớn cấp phát sẵn, nơi các đối tượng sống (chương 3)             |
| Hot path        | Đoạn code chạy cho mọi lệnh; phần phải nhanh nhất                        |

Bảng này chỉ gồm các từ về giao dịch được dùng trong chương này. [Bảng thuật
ngữ](glossary.md) đầy đủ còn giải thích mọi thuật ngữ về cấu trúc dữ liệu, Rust,
lập trình đồng thời và lưu trữ mà các chương sau sử dụng.

## 1.12 Tự thực hành

Hãy chạy ví dụ minh họa và so sánh kết quả với mục 1.10:

```sh
cargo run --example walkthrough
```

Rồi thử sửa nó. Trong [`examples/walkthrough.rs`](../../examples/walkthrough.rs),
đổi `#6` thành `Buy 14 @ 101` thay vì `@ 102`. **Trước khi chạy**, hãy đoán xem lệnh
này khớp với những maker nào, còn dư bao nhiêu, và phần dư nằm chờ ở mức giá nào.
(Các dòng `assert_eq!` sau bước 6 sẽ báo lỗi với con số mới; hãy sửa chúng theo dự
đoán của bạn.)

Unit test ghi lại quy tắc ưu tiên giá-thời gian là một ví dụ ngắn, dễ đọc về API
của engine:

```sh
cargo test price_time_priority_within_a_level_is_fifo
```

**Tiếp theo:** [2. Tổng quan kiến trúc](02-architecture.md)
