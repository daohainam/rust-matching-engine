# 1. Kiến thức cơ bản về sổ lệnh

> *Bản gốc tiếng Anh: [01-order-book-basics.md](../01-order-book-basics.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

> **Chương này dành cho ai:** những người muốn tìm hiểu cách một sàn giao dịch
> hoạt động. Bạn không cần có kiến thức về tài chính. Nếu bạn đã biết "sổ lệnh giới hạn"
> (limit order book) là gì, bạn có thể xem nhanh ví dụ ở cuối chương rồi chuyển
> sang [chương 2](02-architecture.md).

## 1.1 Bộ khớp lệnh là gì?

**Bộ khớp lệnh** (matching engine) là thành phần của sàn giao dịch xử lý một
câu hỏi hàng triệu lần mỗi ngày:

> *"Có người muốn mua, có người muốn bán. Ta có thể khớp lệnh của họ với nhau không, và
> ở mức giá nào?"*

Mọi thứ khác trong kho mã nguồn này (arena, bitset, bảng băm...) đều nhằm trả lời câu
hỏi đó thật **nhanh**, với **thời gian xử lý ổn định**.

## 1.2 Lệnh

**Lệnh** (order) là chỉ thị của một nhà giao dịch. Loại cơ bản nhất là **lệnh
giới hạn** (limit order); các loại lệnh khác mà bộ khớp lệnh này hỗ trợ đều là biến thể
của nó (mục 1.9). Một lệnh giới hạn gồm các thông tin sau:

| Trường     | Ý nghĩa                                                        | Trong mã                  |
|------------|----------------------------------------------------------------|-----------------------------|
| `side`     | Nhà giao dịch muốn **mua** hay **bán**?                        | `Side::Buy` / `Side::Sell`  |
| `price`    | Mức giá *bất lợi nhất* mà nhà giao dịch chấp nhận.                  | `price_tick: u32`           |
| `qty`      | Số lượng đơn vị (cổ phiếu, lô, hợp đồng...).                   | `qty: u64`                  |
| `order_id` | Mã định danh duy nhất của lệnh (ví dụ để hủy lệnh).         | `order_id: u64`             |
| `account`  | Ai đặt lệnh: nhà giao dịch, công ty hoặc thực thể mà bên gọi muốn theo dõi. | `account: u32`              |

"Duy nhất" ở đây nghĩa là *duy nhất trong số các lệnh còn nằm trong sổ*. Bộ khớp lệnh
từ chối lệnh mới nếu ID của nó trùng với một lệnh đang chờ. Khi lệnh đã khớp hết
hoặc bị hủy, bộ khớp lệnh không còn lưu ID đó nên có thể dùng lại. Các sàn giao dịch thực tế thường
không cho phép việc này; trong dự án này, bên gọi (caller) có trách nhiệm tránh dùng lại ID.

Giá là một **giới hạn**, nghĩa là:

- Lệnh **mua** ở giá 100 nói rằng *"tôi trả tối đa 100"*.
- Lệnh **bán** ở giá 100 nói rằng *"tôi bán với giá tối thiểu 100"*.

## 1.3 Sổ lệnh

**Sổ lệnh** (order book) là tập hợp các lệnh đang chờ khớp với lệnh của một đối tác
(counterparty). Sổ có hai phía:

- **Bid**: các lệnh mua đang chờ. Bid tốt nhất (*best bid*) là bid **cao nhất**:
  đó là mức giá cao nhất mà người bán có thể giao dịch ngay.
- **Ask** (còn gọi là offer): các lệnh bán đang chờ. Ask tốt nhất (*best ask*) là
  ask **thấp nhất**: đó là mức giá thấp nhất mà người mua có thể giao dịch ngay.

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
bình thường, bid tốt nhất luôn **thấp hơn** ask tốt nhất. Nếu hai mức giá chạm
nhau, giao dịch sẽ diễn ra ngay. Đó chính là nhiệm vụ của bộ khớp lệnh.

> [!NOTE]
> Chữ **CLOB** trong phần mô tả dự án này là viết tắt của *Central Limit Order
> Book*: một sổ lệnh trung tâm duy nhất, chỉ gồm các lệnh giới hạn, nơi mọi người
> đặt lệnh để giao dịch.

## 1.4 Vượt spread: khi nào có giao dịch

Khi có lệnh mới, trước tiên bộ khớp lệnh kiểm tra xem lệnh đó có thể giao dịch ngay
không. Một lệnh mới **có thể khớp ngay** (marketable, hay nói cách khác là
*vượt spread*) khi:

- lệnh **mua** có giá **≥ ask tốt nhất**, hoặc
- lệnh **bán** có giá **≤ bid tốt nhất**.

```text
   ask tốt nhất = 101

   lệnh MUA mới @ 100  ->  100 < 101   -> chưa khớp ngay được, nằm chờ trong sổ
   lệnh MUA mới @ 101  ->  101 >= 101  -> khớp được, giao dịch với ask ở 101
   lệnh MUA mới @ 105  ->  105 >= 101  -> khớp được, giao dịch với ask ở 101
```

Dòng cuối thể hiện một điểm thường khiến người mới ngạc nhiên: người mua nói
"tối đa 105", nhưng giao dịch lại diễn ra ở giá **101**, tức giá của lệnh đã nằm
sẵn trong sổ. **Giao dịch luôn diễn ra ở giá của lệnh đang chờ.** Vì thế người
mua được giá tốt hơn mức họ chấp nhận (*price improvement*).

### Maker và taker

Mỗi giao dịch có hai lệnh tham gia:

- **Maker** là lệnh đã nằm sẵn trong sổ. Lệnh này *cung cấp* thanh khoản
  (liquidity).
- **Taker** là lệnh vừa đến và *lấy đi* thanh khoản đó. Nó còn được gọi là
  **aggressor** (bên chủ động), và phía của nó là **aggressor side** của giao
  dịch: một giao dịch mà người mua vượt qua spread được gọi là "do bên mua khởi
  xướng" (*buyer-initiated*).

Bạn sẽ gặp hai từ này khắp nơi trong mã (`maker_order_id`, `taker_order_id`,
`taker_side`, `match_incoming`).

## 1.5 Ưu tiên giá, rồi đến thời gian

Nếu có nhiều lệnh đang chờ thì lệnh nào được xử lý trước? Quy tắc mà hầu hết
các sàn (và bộ khớp lệnh này) áp dụng là **ưu tiên giá, rồi đến thời gian**
(price-time priority):

1. **Ưu tiên giá.** Lệnh ở mức giá tốt nhất được khớp trước. Với người mua, đó là ask thấp
   nhất; với người bán, đó là bid cao nhất.
2. **Sau đó là thời gian.** Trong các lệnh **cùng một giá**, lệnh đến **trước** được
   khớp trước (FIFO: vào trước, ra trước).

```text
   Mức giá 101 (ask):   [ #2 bán 3 ]  ->  [ #3 bán 4 ]
                          cũ nhất            mới nhất
                          khớp trước         khớp sau
```

Vì vậy, mỗi mức giá trong bộ khớp lệnh là một **hàng đợi** chứ không chỉ là một con số
(xem [chương 3](03-order-pool.md)).

## 1.6 Khớp một phần

Lệnh mới có thể có số lượng lớn hơn lượng đang chờ trong sổ. Khi đó lệnh chỉ được
khớp **một phần**:

- Nếu taker có số lượng lớn hơn maker, maker được khớp hết và rời khỏi sổ; taker
  tiếp tục khớp với lệnh đang chờ kế tiếp.
- Nếu taker có số lượng nhỏ hơn maker, taker được khớp hết, còn maker ở lại trong
  sổ với số lượng còn lại ít hơn.
- Nếu hai bên có đúng cùng số lượng, **cả hai** đều khớp hết cùng lúc: maker rời
  khỏi sổ và taker cũng hoàn tất. (Bộ khớp lệnh phát một sự kiện `OrderFilled` cho từng lệnh.)
- Nếu sau khi đã giao dịch với mọi lệnh có thể mà taker vẫn còn dư số lượng, thì
  **phần dư nằm chờ** trong sổ như một lệnh đang chờ mới.

## 1.7 Hủy lệnh

Nhà giao dịch có thể hủy một lệnh đang chờ bất cứ lúc nào bằng `order_id` của lệnh.
Bộ khớp lệnh phải tìm ra lệnh đó *mà không quét cả sổ*, gỡ nó ra khỏi giữa hàng đợi, và
nếu đó là lệnh cuối cùng ở mức giá ấy thì phải xóa luôn mức giá khỏi sổ. Làm được
việc này trong O(1) là một trong những thách thức thiết kế chính (xem
[chương 3 đến 5](03-order-pool.md)).

Nhà giao dịch cũng có thể **giảm** số lượng của một lệnh đang chờ. Lệnh vẫn giữ
nguyên vị trí trong hàng đợi: giảm số lượng không giúp lệnh vượt lên trước các
lệnh khác, nên vẫn giữ được thứ tự ưu tiên. Nếu muốn tăng số lượng hoặc đổi giá,
nhà giao dịch phải hủy lệnh cũ rồi gửi lệnh mới. Lệnh mới sẽ xếp vào cuối hàng đợi.

Đôi khi cần hủy toàn bộ lệnh của một nhà giao dịch cùng lúc, chẳng hạn khi mất
kết nối (nhiều sàn áp dụng cơ chế *cancel on disconnect*), hoặc khi hệ thống
quản lý rủi ro yêu cầu dừng giao dịch. **Hủy hàng loạt** (mass cancel) cho phép
hủy mọi lệnh của một tài khoản, hoặc của cả sổ lệnh khi kết thúc phiên hay
tạm dừng giao dịch.

### Giao dịch với chính mình

Một nhà giao dịch lớn thường có cả lệnh mua lẫn lệnh bán trong sổ cùng lúc, xuất
phát từ các chiến lược khác nhau. Nếu lệnh mua của họ khớp với lệnh bán của chính
họ, một giao dịch vẫn được ghi nhận dù không có tài sản nào chuyển từ chủ này
sang chủ khác; tuy nhiên, thị trường lại nhìn thấy hoạt động đó như một giao dịch
thật. Cố ý làm vậy gọi là **giao dịch giả** (wash trading), một hành vi thao túng
thị trường bất hợp pháp. Vì thế các sàn cung cấp cơ chế **chống tự giao dịch**
(self-trade prevention): nếu lệnh mới sắp khớp với lệnh đang chờ của cùng tài
khoản, giao dịch bị chặn. Nhà giao dịch có thể chọn cách xử lý tiếp theo: hủy lệnh
đang chờ, hủy lệnh mới đến hoặc hủy cả hai.

## 1.8 Tick: giá là những số nguyên nhỏ

Giá thực tế thường có dạng như `101.25`. Số dấu phẩy động không biểu diễn chính
xác được hầu hết phân số thập phân, nên có thể cho kết quả bất ngờ
(`0.1 + 0.2 != 0.3`). Điều đó không thể chấp nhận khi giao dịch tiền bạc, vì vậy
các sàn dùng **tick nguyên**. Một *tick* là bước giá nhỏ nhất được phép.

```text
   kích thước tick = 0.25    tick 0   -> giá 100.00   (nếu ta chọn mốc lệch (offset) là 100.00)
                             tick 1   -> giá 100.25
                             tick 2   -> giá 100.50
                             ...
                             tick 4095-> giá 1123.75
```

Bộ khớp lệnh này **chỉ làm việc với tick**: `price_tick` là một `u32` từ `0` đến nhỏ
hơn số tick được cấu hình. Mặc định có 4096 tick, trừ khi bạn chọn số khác
bằng `Engine::with_ticks` (tối đa khoảng 16.7 triệu). Ví dụ ở trên cần 4096 tick
để lên tới 1123.75; một cổ phiếu quanh mức 100.00, giao dịch theo bước 0.01, cần
hơn 10,000 tick. Việc đổi qua lại giữa giá thực tế và tick
(`price = offset + tick * tick_size`) là **trách nhiệm của bên gọi**.

Các ví dụ trong các chương đều dùng 4096 tick mặc định.

Vì sao điều này quan trọng với thiết kế? Khi phạm vi giá không quá lớn và các
mức giá nằm gần nhau, ta có thể dùng **mảng đánh chỉ số theo tick** thay cho cây
hay bảng băm. `levels[tick]` chỉ cần một lần đọc bộ nhớ. Đây là ý tưởng nền tảng
của toàn bộ thiết kế.

## 1.9 Bộ khớp lệnh này hỗ trợ gì (và không hỗ trợ gì)

| Có hỗ trợ                                              | Chưa hỗ trợ                                            |
|--------------------------------------------------------|--------------------------------------------------------|
| Lệnh giới hạn, immediate-or-cancel, thị trường (market) và post-only | Lệnh dừng (stop), fill-or-kill, lệnh trong ngày |
| Khớp một phần và khớp đủ                               | Lệnh tảng băng (iceberg, có phần số lượng ẩn)          |
| Cho phần chưa khớp nằm chờ trong sổ                    | Tăng số lượng hoặc đổi giá ngay tại chỗ (có thể hủy rồi đặt lệnh mới) |
| Hủy theo `order_id`; giảm số lượng mà vẫn giữ ưu tiên  | Giới hạn theo tài khoản (vị thế, số lệnh đang mở)      |
| Tài khoản, chống tự giao dịch, hủy hàng loạt           | Phiên giao dịch và tạm dừng giao dịch (hiện chỉ có `cancel_all`) |
| Ưu tiên giá, rồi đến thời gian                         | Nhiều mã giao dịch trong một `Engine` (mỗi mã một bộ khớp lệnh) |
| Truy vấn độ sâu và tra cứu lệnh                        | Phiên đấu giá, cơ chế ngắt giao dịch, phí            |
| Số tick chọn riêng cho từng bộ khớp lệnh, tối đa 2^24        | Giá không phải là một số tick nguyên                   |
| Giới hạn rủi ro trước giao dịch: số lượng tối đa, biên độ giá | Rủi ro theo tài khoản (vị thế, tín dụng)        |

### Giới hạn rủi ro: ngăn lỗi bấm nhầm (fat finger)

Nhà giao dịch nhập 10000 thay vì 100, hoặc đặt giá mua cao gấp mười lần, có thể
gây thiệt hại thật chỉ trong vài microgiây: lệnh đó có thể khớp với mọi lệnh mà
nó gặp. Vì vậy, các sàn kiểm tra mọi lệnh theo **giới hạn rủi ro trước giao
dịch** trước khi cho phép khớp. Bộ khớp lệnh này có hai giới hạn:

- **số lượng tối đa của một lệnh**;
- **biên độ giá (price band)**: một lệnh chỉ được khớp ở mức giá cách giá tốt
  nhất phía đối diện tối đa vài tick. Lệnh mua giới hạn ở 150 khi ask tốt nhất
  là 100 sẽ bị từ chối, còn lệnh thị trường dừng ở ranh giới biên độ thay vì
  quét sạch cả sổ.

Các giới hạn này là một phần trạng thái của bộ khớp lệnh và có thể thay đổi trong ngày,
chẳng hạn khi thị trường trở nên biến động mạnh.

### Các loại lệnh

Các loại lệnh tuân theo cùng thứ tự ưu tiên khi khớp: mức giá tốt nhất trước, rồi
đến lệnh cũ nhất. Chúng khác nhau ở việc có được phép khớp hay không và cách xử
lý phần số lượng chưa khớp ngay:

| Loại | Giao dịch khi vừa đến | Số lượng còn dư | Trong mã |
|---|---|---|---|
| **Giới hạn** (limit) | ở giá của nó hoặc tốt hơn | nằm chờ trong sổ | `OrderType::Limit` |
| **Immediate-or-cancel** (IOC) | ở giá của nó hoặc tốt hơn | hết hạn, không bao giờ nằm chờ | `OrderType::ImmediateOrCancel` |
| **Thị trường** (market) | ở bất kỳ giá nào | hết hạn, không bao giờ nằm chờ | `OrderType::Market` |
| **Post-only** | không bao giờ: bị từ chối nếu sắp giao dịch | nằm chờ trong sổ | `OrderType::PostOnly` |

Lệnh **thị trường** có nghĩa là "mua 10 ngay, giá bao nhiêu cũng được". Lệnh này
không có mức giá riêng, nên có thể khớp qua nhiều mức giá. Vì vậy, các sàn thật
bảo vệ lệnh thị trường bằng biên độ giá (price band); bộ khớp lệnh này cũng làm vậy nếu
đã cấu hình biên độ (xem *Giới hạn rủi ro* ở trên).
Lệnh **post-only** dành cho các nhà tạo lập thị trường (market maker), những người
muốn chắc chắn rằng lệnh của họ chỉ *bổ sung* thanh khoản cho sổ (và thường được
hưởng phí thấp hơn nhờ vậy): nếu lệnh sắp khớp ngay khi vừa đến, nó sẽ bị từ chối.

### Lệnh sai thì sao?

Một số lệnh bị từ chối trước khi được đưa vào sổ. Bộ khớp lệnh phát sự kiện
`OrderRejected` nói rõ lý do, và không có gì khác thay đổi:

| Lệnh có...                                                                        | Lý do được báo       |
|-----------------------------------------------------------------------------------|----------------------|
| số lượng bằng 0                                                                   | `ZeroQuantity`       |
| ID là `u64::MAX` (dành riêng cho nội bộ, xem [chương 5](05-hash-map.md))          | `ReservedOrderId`    |
| tick nằm ngoài phạm vi của bộ khớp lệnh (mặc định là từ 4096 trở lên)                   | `PriceOutOfRange`    |
| cùng ID với một lệnh vẫn đang chờ                                                 | `DuplicateOrderId`   |
| số lượng vượt quá số lượng tối đa cho một lệnh của bộ khớp lệnh                         | `QuantityAboveLimit` |
| mức giá cách giá tốt nhất phía đối diện quá biên độ cho phép    | `PriceOutsideBand`   |
| loại post-only với mức giá có thể khớp ngay                                  | `PostOnlyWouldTrade` |

Một lệnh cũng có thể được chấp nhận và giao dịch bình thường, nhưng rồi **không
còn chỗ trong sổ** để phần dư nằm chờ. Lúc đó nó bị từ chối với lý do
`BookFull` (chương 6 giải thích khi nào điều này xảy ra).

Phạm vi tính năng được giới hạn có chủ ý để mã nguồn đủ ngắn để đọc trọn trong
một lần.
[Chương 11](11-limitations-and-exercises.md) liệt kê các ý tưởng mở rộng.

## 1.10 Một ví dụ hoàn chỉnh

Hãy theo dõi kỹ ví dụ này; ta sẽ dùng lại ở
[chương 6](06-matching-algorithm.md) để minh họa những gì diễn ra bên trong
bộ khớp lệnh. Các kết quả dưới đây đều được tạo bởi triển khai thực tế của bộ khớp lệnh và bạn có thể tự chạy
lại:

```sh
cargo run --example walkthrough
```

Chương trình ([`examples/walkthrough.rs`](../../examples/walkthrough.rs)) lần
lượt gửi các lệnh dưới đây, rồi in sự kiện và trạng thái sổ sau mỗi bước. Sổ in
ra tổng số lượng ở mỗi mức giá; các sơ đồ bên dưới bổ sung hàng đợi của từng
lệnh, cấu trúc sẽ được xây dựng trong chương 3 và 6.

**Bước 1 đến 5: năm lệnh lần lượt được gửi đến. Không lệnh nào khớp ngay nên
tất cả đều nằm chờ trong sổ.**

| # | Lệnh đến               | Có thể khớp ngay? | Kết quả      |
|---|------------------------|-------------|--------------|
| 1 | `#1` BÁN 5 @ 102       | chưa có bid | nằm chờ      |
| 2 | `#2` BÁN 3 @ 101       | chưa có bid | nằm chờ      |
| 3 | `#3` BÁN 4 @ 101       | chưa có bid | nằm chờ, **đứng sau** `#2` |
| 4 | `#4` MUA 10 @ 99       | 99 < ask tốt nhất 101 | nằm chờ  |
| 5 | `#5` MUA 6 @ 100       | 100 < ask tốt nhất 101 | nằm chờ |

Sổ lệnh lúc này như sau:

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

**Bước 6: lệnh `#6` MUA 14 @ 102 được gửi đến.** Lệnh có thể khớp ngay
(102 ≥ 101). Bộ khớp lệnh duyệt phía ask, bắt đầu từ mức giá thấp nhất:

| Lần khớp | Với          | Giá   | SL  | Vì sao                                               |
|----------|--------------|-------|-----|------------------------------------------------------|
| 1        | `#2` (maker) | 101   | 3   | cũ nhất ở giá tốt nhất; `#2` khớp hết                |
| 2        | `#3` (maker) | 101   | 4   | lệnh kế tiếp trong hàng đợi FIFO; `#3` khớp hết      |
| 3        | `#1` (maker) | 102   | 5   | mức 101 đã hết lệnh, chuyển lên 102; `#1` khớp hết   |

Tổng khớp được là 3 + 4 + 5 = 12. Lệnh có số lượng 14 nên **còn dư 2 đơn vị**.
Không còn ask nào, vì vậy phần dư **nằm chờ như một bid ở giá 102**:

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

Chuỗi **sự kiện** chính xác mà bộ khớp lệnh phát ra cho bước 6 là:

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

Mỗi `Trade` đều cho biết bên chủ động (`taker_side: Buy`: người mua đã vượt
spread) và có một `trade_id`. Đây là ba giao dịch đầu tiên của sổ lệnh này, nên
chúng được đánh số 1, 2 và 3.

Sự kiện là cách bộ khớp lệnh nói chuyện với thế giới bên ngoài; xem
[chương 7](07-events.md).

## 1.11 Thuật ngữ

| Thuật ngữ       | Ý nghĩa                                                                  |
|-----------------|--------------------------------------------------------------------------|
| Bid             | Một lệnh mua đang chờ                                                    |
| Ask / Offer     | Một lệnh bán đang chờ                                                    |
| Best bid / ask  | Bid cao nhất / ask thấp nhất, tức mức giá tốt nhất của mỗi phía           |
| Spread          | Ask tốt nhất trừ bid tốt nhất                                            |
| Lệnh giới hạn   | "Giao dịch ở mức giá này hoặc tốt hơn"                                   |
| Lệnh thị trường | "Giao dịch ngay, ở bất kỳ giá nào"; phần không giao dịch được ngay sẽ hết hạn |
| IOC             | Immediate-or-cancel: một lệnh giới hạn có phần dư hết hạn thay vì nằm chờ |
| Post-only       | Một lệnh giới hạn bị từ chối nếu nó sẽ giao dịch ngay khi vừa đến        |
| Tài khoản (account) | Tài khoản đặt lệnh; cơ chế chống tự giao dịch và hủy hàng loạt dùng thông tin này |
| Tự giao dịch (self-trade) | Giao dịch giữa hai lệnh của cùng một tài khoản; có thể được ngăn chặn |
| Biên độ giá (price band) | Giới hạn mức giá lệnh được phép khớp so với giá tốt nhất phía đối diện |
| Marketable      | Có thể giao dịch ngay với phía đối diện                                  |
| Maker           | Lệnh đang chờ được khớp trong một giao dịch                              |
| Taker           | Lệnh vừa đến trong một giao dịch                                         |
| Mức giá (price level) | Tất cả các lệnh đang chờ ở cùng một giá                            |
| Độ sâu (depth)  | Tổng số lượng có sẵn ở một mức giá (hoặc qua nhiều mức)                  |
| Fill            | Một lần khớp; *khớp một phần* nghĩa là vẫn còn số lượng dư               |
| Tick            | Bước giá nhỏ nhất; bộ khớp lệnh này dùng tick nguyên, mặc định từ 0 đến 4095  |
| Ưu tiên giá-thời gian | Mức giá tốt hơn được ưu tiên trước, rồi đến lệnh đến sớm hơn       |
| Arena           | Một mảng lớn cấp phát sẵn, nơi lưu các đối tượng (chương 3)             |
| Hot path        | Đoạn mã chạy cho mọi lệnh; phần phải nhanh nhất                        |

Bảng này chỉ gồm các thuật ngữ giao dịch dùng trong chương. [Bảng thuật
ngữ](glossary.md) còn giải thích đầy đủ các thuật ngữ về cấu trúc dữ liệu, Rust,
lập trình đồng thời và lưu trữ được dùng ở các chương sau.

## 1.12 Tự thực hành

Chạy ví dụ minh họa và so sánh kết quả với mục 1.10:

```sh
cargo run --example walkthrough
```

Tiếp theo, thử sửa chương trình. Trong [`examples/walkthrough.rs`](../../examples/walkthrough.rs),
đổi `#6` thành `Buy 14 @ 101` thay vì `@ 102`. **Trước khi chạy**, hãy đoán lệnh
này sẽ khớp với những maker nào, còn dư bao nhiêu và phần dư sẽ nằm chờ ở mức giá
nào. (Các dòng `assert_eq!` sau bước 6 sẽ báo lỗi vì kết quả đã thay đổi; hãy sửa
chúng theo dự đoán của bạn.)

Kiểm thử đơn vị ghi lại quy tắc ưu tiên giá-thời gian là ví dụ ngắn, dễ đọc về API của
bộ khớp lệnh:

```sh
cargo test price_time_priority_within_a_level_is_fifo
```

**Tiếp theo:** [2. Tổng quan kiến trúc](02-architecture.md)
