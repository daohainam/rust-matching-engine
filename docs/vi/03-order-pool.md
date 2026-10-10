# 3. Order pool: kho chứa lệnh (arena, free list và hàng đợi FIFO)

> *Bản gốc tiếng Anh: [03-order-pool.md](../03-order-pool.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Mã nguồn: [`src/pool.rs`](../../src/pool.rs)

Mô-đun này trả lời hai câu hỏi:

1. **Các lệnh được lưu ở đâu trong bộ nhớ?** Trong một mảng lớn được cấp phát
   sẵn (một *arena*).
2. **Làm sao giữ đúng thứ tự đến của các lệnh ở cùng mức giá?** Dùng danh sách
   liên kết với chỉ số mảng làm liên kết (danh sách liên kết *intrusive*).

## 3.1 Bài toán

Mỗi mức giá cần một hàng đợi lệnh: lệnh mới được thêm vào cuối hàng, lệnh được
khớp lấy từ đầu hàng, còn lệnh bị hủy có thể bị xóa ở *bất kỳ vị trí nào*. Cách
làm quen thuộc trong Rust là dùng `VecDeque<Order>` hoặc `LinkedList<Order>`,
nhưng:

- `VecDeque` xóa phần tử ở giữa mất O(n) (mọi phần tử phía sau phải dịch lên).
- `LinkedList` cấp phát một nút trên heap cho mỗi lệnh, và các nút thường nằm
  rải rác khắp bộ nhớ.
- Cả hai đều cấp phát trên hot path (đoạn mã chạy cho mọi lệnh).

Mục tiêu là: **thêm vào cuối O(1), lấy ra từ đầu O(1), xóa ở giữa O(1), không
cấp phát.**

## 3.2 Ý tưởng 1: arena

Một **arena** là một mảng lớn, chỉ cấp phát một lần. Các đối tượng được truy cập
bằng **chỉ số** (index) trong mảng thay vì bằng con trỏ.

```text
   orders: Box<[Order]>   (độ dài = capacity, cố định mãi mãi)

   chỉ số:  0      1      2      3      4      5     ...   capacity-1
          +------+------+------+------+------+------+-----+------+
          |Order |Order |Order |Order |Order |trống | ... |trống |
          +------+------+------+------+------+------+-----+------+
```

Vì sao dùng chỉ số thay vì con trỏ?

- Một chỉ số `u32` chiếm **4 byte**, còn con trỏ chiếm **8**. Mỗi lệnh có hai
  liên kết, nên tiết kiệm được 8 byte cho mỗi lệnh, và nhờ đó nhiều lệnh hơn nằm
  vừa trong cache.
- Các lệnh nằm **liền nhau** trong bộ nhớ, giúp prefetcher (bộ nạp trước)
  của CPU hoạt động hiệu quả.
- Không cần `unsafe` hay tham số lifetime: chỉ số chỉ là một con số, nên tránh
  được các ràng buộc của borrow checker (bộ kiểm tra mượn). Một cấu trúc tự
  tham chiếu gồm các tham chiếu `&mut Order` không thể biểu diễn bằng Rust an toàn.

Đổi lại, **trình biên dịch không kiểm tra chỉ số còn hợp lệ hay không.** Giả
sử có đoạn mã giữ chỉ số ô `7` cho lệnh `#42`, rồi `#42` bị hủy và ô 7 được
cấp cho lệnh mới `#99`. Chỉ số cũ vẫn "dùng được": `pool.get(7)` trả về dữ liệu
của `#99` mà không báo lỗi. Đây là
phiên bản "chỉ số" của *con trỏ treo* (dangling pointer), thường gọi là **chỉ số
cũ** (stale index). Nó không thể gây ra hành vi không xác định (undefined
behavior, UB), vì ô nhớ luôn chứa một `Order` hợp lệ, nhưng nó có thể âm thầm
đọc hoặc sửa nhầm lệnh.

Bộ khớp lệnh tránh lỗi này nhờ một quy ước: không bao giờ giữ chỉ số qua nhiều lời gọi.
Các chỉ số duy nhất được lưu qua nhiều lần gọi là các mục trong bảng băm và các
liên kết của hàng đợi; cả hai đều bị xóa ngay khi ô được giải phóng. Chỉ `free`
kiểm tra cờ `in_use` của ô (mục 3.7), còn `get` thì không. Nếu cần an toàn hơn,
có thể dùng **chỉ số kèm số thế hệ** (generational index): mỗi ô lưu một bộ đếm tăng
lên mỗi lần được dùng lại, và bộ đếm đó cũng trở thành một phần của chỉ số, nhờ
đó chỉ số cũ không còn hợp lệ.

## 3.3 Bản ghi `Order`

```rust
#[repr(C)]
pub struct Order {
    pub order_id: u64,        // user's id
    pub remaining_qty: u64,   // not yet executed
    pub sequence: u64,        // arrival counter (metadata)
    pub price_tick: u32,      // which price level it rests at
    pub side: Side,           // Buy or Sell (1 byte)
    pub next_idx: u32,        // next order in the same price queue (towards the tail)
    pub prev_idx: u32,        // previous order in the same price queue (towards the head)
    pub in_use: bool,         // is this slot currently holding a live order?
    pub account: u32,         // who owns the order
}
```

`#[repr(C)]` giữ các trường theo đúng thứ tự khai báo. Trình biên dịch chèn thêm
*padding* (byte đệm) để mỗi trường được căn chỉnh (aligned). Đo bằng `offset_of!`
ta được:

```text
 byte  0         8        16        24   28   32        36        40   41   44        48
       +---------+---------+---------+----+----+--------+--------+----+----+---------+
       |order_id |remaining|sequence |tick|side|next_idx|prev_idx|in_ |    | account |
       |  u64    | _qty u64|  u64    |u32 | u8 |  u32   |  u32   |use |    |   u32   |
       +---------+---------+---------+----+----+--------+--------+----+----+---------+
                                            ^^^ 3 byte padding          ^^^ 3 byte padding
       tổng size_of::<Order>() = 48 byte
```

Hai khoảng trống đó xuất hiện vì mỗi kiểu dữ liệu có một **yêu cầu căn chỉnh**
(alignment): `u32` phải bắt đầu ở địa chỉ chia hết cho 4, `u64` ở địa chỉ chia
hết cho 8.

- **3 byte sau `side`.** `side` chiếm một byte, ở offset 28, nên byte trống kế
  tiếp là 29. `next_idx` là một `u32` và phải bắt đầu ở địa chỉ chia hết cho 4,
  nên trình biên dịch bỏ qua các byte từ 29 đến 31 và đặt nó ở 32.
- **3 byte sau `in_use`.** Cũng y như vậy: `in_use` chiếm một byte, ở 40, và
  `account` phải bắt đầu ở địa chỉ chia hết cho 4, nên nó được đặt ở 44 và kết
  thúc đúng ở 48. Đó cũng là một bội số của 8, đúng như yêu cầu: `Order` chứa
  các `u64`, nên cả struct được căn chỉnh 8 byte và kích thước của nó được làm tròn
  lên bội số của 8. Nhờ vậy, trong một mảng các lệnh, mọi phần tử (và mọi `u64`
  bên trong nó) cũng đều bắt đầu ở địa chỉ được căn chỉnh đúng.

Trường `account` được thêm vào sau cùng mà không làm tăng kích thước: trước đó,
struct cũng kết thúc ở byte 41 và được làm tròn lên 48, với 7 byte padding ở
cuối. `account` tận dụng chính khoảng padding này. Một kiểm thử
(`the_account_fits_in_existing_padding` trong `pool.rs`) kiểm tra kích thước
này, nhờ đó mọi trường mới làm struct lớn hơn đều khiến kiểm thử thất bại.

Vậy tại sao cần dùng `repr(C)`? Nếu không có nó, Rust dùng bố cục mặc định và có
thể **sắp xếp lại thứ tự các trường** để giảm padding. Trong trường hợp này,
tổng các trường chiếm 42 byte nhưng kích thước vẫn được làm tròn lên 48 byte.
(Trước khi có `account`, bố
cục mặc định chiếm 40 byte, còn `repr(C)` cần 48.) `repr(C)` tạo ra bố cục cố
định, dễ đoán: bố cục trong hình phía trên được bảo đảm, không phụ thuộc vào chi tiết có
thể thay đổi của trình biên dịch. Nhờ vậy, ta có thể vẽ, đo và giải thích bố cục.

> [!NOTE]
> Chú thích trong mã nguồn nói rằng lệnh có kích thước "sized to fit within a
> single 64-byte cache line" (vừa trong một cache line 64 byte). Điều đó đúng
> theo nghĩa 48 ≤ 64, nhưng 48 không chia hết 64, nên trong một mảng xếp sát
> nhau, nhiều lệnh sẽ **nằm vắt ngang hai cache line** (nếu mảng bắt đầu đúng ở
> ranh giới một line, lệnh số 1 chiếm các byte 48 đến 95, vượt qua ranh giới ở
> byte 64). [Chương 11](11-limitations-and-exercises.md) có liệt kê hai thí
> nghiệm: bỏ trường `sequence`, vốn không có chỗ nào đọc tới, để thu bản ghi
> xuống 40 byte, hoặc căn chỉnh nó theo 64 byte để mỗi lệnh chiếm đúng một cache
> line.

## 3.4 Ý tưởng 2: free list (ngăn xếp các ô trống)

Làm sao tìm một ô chưa dùng trong O(1)? Giữ một **ngăn xếp chứa chỉ số của các ô trống**:

```rust
pub struct OrderPool {
    orders: Box<[Order]>,
    free_stack: Box<[u32]>,   // free_stack[..free_top] are the free slot indices
    free_top: usize,          // number of free slots
}
```

- **allocate** = lấy (pop) chỉ số trên cùng của ngăn xếp.
- **free** = đẩy (push) chỉ số trở lại ngăn xếp.

Cả hai chỉ tốn vài lệnh máy. Đây là một lần chạy thử với `capacity = 4`:

```text
 new(4)           free_stack = [3, 2, 1, 0]   free_top = 4     (chưa ô nào được dùng)
                                        ^top

 allocate()  -> 0 free_stack = [3, 2, 1, 0]   free_top = 3     (ô 0 đang dùng)
 allocate()  -> 1 free_stack = [3, 2, 1, 0]   free_top = 2     (ô 0,1 đang dùng)
                                  ^top        (các phần tử phía trên đỉnh là dữ liệu cũ, bị bỏ qua)

 free(0)          free_stack = [3, 2, 0, 0]   free_top = 3     (ô 0 được đẩy trở lại)
 allocate()  -> 0 free_stack = [3, 2, 0, 0]   free_top = 2     (ô 0 được dùng lại đầu tiên)
```

Ngăn xếp được điền ngược (`capacity-1, ..., 1, 0`) để lần `allocate` đầu tiên
trả về ô `0`, lần kế tiếp trả về ô `1`, rồi tiếp tục theo thứ tự đó. (Cách này
chỉ nhằm giúp thứ tự chỉ số dễ nhìn hơn, như chú thích trong mã nguồn đã nêu.)

Vì đây là một *ngăn xếp* (vào sau, ra trước, LIFO), ô được giải phóng gần nhất
sẽ được dùng lại trước. Vùng nhớ ở ô đó vừa được truy cập nên có thể vẫn còn
trong cache, một lợi thế nhỏ mà không tốn thêm chi phí.

`live_count()` đơn giản là `capacity - free_top`.

### Khi cấp phát thất bại

Nếu `free_top == 0`, arena đã đầy và `allocate` trả về `None`. Bộ khớp lệnh phát sự
kiện `OrderRejected` với lý do `BookFull` (chỉ khi phần còn lại của lệnh cần nằm
chờ trong sổ; lệnh khớp hết thì không cần ô nào).

## 3.5 Ý tưởng 3: danh sách liên kết đôi intrusive

"Intrusive" nghĩa là các liên kết của danh sách được lưu **ngay bên trong phần
tử** (`next_idx`, `prev_idx` trong `Order`) thay vì trong một đối tượng nút
riêng. Nói cách khác, bản thân lệnh *chính là* nút của danh sách, nên không cần
cấp phát thêm.

Mỗi tick giá có một bộ mô tả rất nhỏ, `PriceLevel`:

```rust
pub struct PriceLevel {
    pub head: u32,       // oldest order (first to trade), or NIL if empty
    pub tail: u32,       // newest order (last to trade), or NIL if empty
    pub count: u32,      // number of orders at this level
    pub total_qty: u64,  // sum of remaining_qty of all orders (O(1) depth queries)
}
```

Hàng đợi tại tick ask 101 trong ví dụ xuyên suốt (`#2` rồi đến `#3`, nằm ở các ô
arena 1 và 2):

```text
   ask_levels[101]:  head=1  tail=2  count=2  total_qty=7
                       |        |
                       v        v
   slot 1  [ #2 qty 3 | prev=NIL | next=2  ]  <---->  slot 2  [ #3 qty 4 | prev=1 | next=NIL ]
           (head, cũ nhất)                                     (tail, mới nhất)
```

### `NIL`: "không có liên kết"

```rust
pub const NIL: u32 = u32::MAX;
```

`NIL` đóng vai trò của con trỏ null. Ta dùng một giá trị đánh dấu đặc biệt
(sentinel) thay vì `Option<u32>`, vì `Option<u32>` chiếm **8 byte** (4 byte cho
giá trị cộng với một thẻ đánh dấu kèm padding để căn chỉnh), tức là làm kích thước
của cả hai liên kết tăng gấp đôi. Đó cũng là lý do `OrderPool::new` yêu cầu
`capacity < u32::MAX`: giá trị `u32::MAX` được giữ riêng.

### Thao tác 1: `push_back` (một lệnh mới vào cuối hàng đợi)

Thêm ô `idx` vào cuối hàng đợi (tail). Có hai trường hợp:

```text
 TRƯỜNG HỢP A: mức giá đang rỗng (tail == NIL)

   trước:   head=NIL tail=NIL
   push(X)
   sau:     head=X   tail=X        X.prev=NIL  X.next=NIL


 TRƯỜNG HỢP B: mức giá đã có lệnh

   trước:   head=A  ...  tail=C
            A <-> B <-> C
   push(X)
      1. C.next = X
      2. X.prev = C
      3. X.next = NIL
      4. tail   = X
   sau:     A <-> B <-> C <-> X       head=A  tail=X
```

Cập nhật các bộ đếm: `count += 1` và `total_qty += X.remaining_qty`. Phép cộng này
dùng `checked_add` (xem 3.7).

### Thao tác 2: `remove` (xóa ở đầu, giữa hoặc cuối)

Vì mỗi nút đều biết cả hai nút lân cận, việc xóa không bao giờ phải tìm kiếm:

```text
 remove(B) from   A <-> B <-> C           (ở giữa)
    A.next = C ; C.prev = A               =>   A <-> C

 remove(A) from   A <-> B <-> C           (đầu)
    prev là NIL nên head = B ; B.prev = NIL  =>   B <-> C   head=B

 remove(C) from   A <-> B <-> C           (cuối)
    next là NIL nên tail = B ; B.next = NIL  =>   A <-> B   tail=B
```

Đoạn mã dưới đây thực hiện các bước đó:

```rust
if prev != NIL { pool.get_mut(prev).next_idx = next; } else { self.head = next; }
if next != NIL { pool.get_mut(next).prev_idx = prev; } else { self.tail = prev; }
```

Sau đó, các liên kết của chính nút bị xóa được đặt lại thành `NIL`, còn `count`
và `total_qty` bị giảm đi (bằng phép trừ có kiểm tra, xem 3.7).

Đây chính là lý do danh sách phải liên kết **đôi**: để hủy `B` trong O(1), ta
phải truy cập được `A` (để cập nhật `A.next`) mà không cần đi từ đầu danh sách.

### Thao tác 3: `reduce_qty` (khớp một phần)

Khi một maker được khớp một phần, nó vẫn ở đầu hàng. Chỉ có các con số thay đổi:

```text
   order.remaining_qty -= delta
   level.total_qty     -= delta
```

Thứ tự trong hàng đợi (và do đó cả ưu tiên về thời gian của maker) không thay
đổi. Một lệnh đã khớp một phần **giữ nguyên vị trí** ở đầu hàng.

`Engine::reduce_order` cũng dùng thao tác này khi nhà giao dịch giảm số lượng
của lệnh đang chờ (chương 6). Lệnh có thể nằm ở bất cứ vị trí nào trong hàng
đợi, không chỉ ở đầu hàng, nhưng vẫn giữ nguyên vị trí vì `reduce_qty` không
thay đổi các liên kết.

## 3.6 Các bất biến (những điều luôn phải đúng)

Đây là các quy tắc mà phần còn lại của bộ khớp lệnh dựa vào. Nếu bất kỳ quy tắc nào bị
vi phạm, sổ lệnh đã hỏng.

| # | Bất biến | Được duy trì bởi |
|---|---|---|
| 1 | `level.head == NIL` ⇔ `level.tail == NIL` ⇔ `level.count == 0` ⇔ `level.total_qty == 0` | `push_back`, `remove` |
| 2 | `level.total_qty` = tổng `remaining_qty` của mọi lệnh trong hàng đợi. (Vế "`total_qty == 0` ⇔ rỗng" của bất biến 1 còn dựa vào việc bộ khớp lệnh không bao giờ để lệnh có số lượng bằng 0 nằm chờ trong sổ: `submit_order` từ chối `qty == 0`, và `reduce_order` hủy lệnh thay vì giảm nó về 0; bản thân `PriceLevel` không kiểm tra.) | `push_back` (+), `remove` (−), `reduce_qty` (−) |
| 3 | Đi theo `head -> next_idx -> ...` sẽ thăm đúng `count` lệnh và kết thúc ở `tail` | `push_back`, `remove` |
| 4 | `a.next_idx == b` ⇔ `b.prev_idx == a` | `push_back`, `remove` |
| 5 | Một ô nằm trong free stack ⇔ `in_use == false` | `allocate`, `free` |
| 6 | Mọi lệnh đang hoạt động đều nằm trong đúng một hàng đợi **và** có một mục trong `id_map` | `Engine` |

## 3.7 Các kiểm tra bảo vệ

Kho chứa lệnh xử lý ba tình huống bất thường sau; cả ba đều được thể hiện rõ trong mã:

**Tràn số khi chèn (overflow).** `push_back` dùng `checked_add` cho `total_qty`.
Nếu chương trình phía khách bị lỗi hoặc cố ý gửi số lượng khiến tổng ở mức giá vượt quá
`u64::MAX`, `push_back` trả về `false` *mà không thay đổi trạng thái nào*.
Phương thức này được đánh dấu `#[must_use]`, và bộ khớp lệnh phản ứng bằng
cách xóa mục `id_map` của lệnh, giải phóng ô, rồi phát sự kiện `OrderRejected`
với lý do `LevelQuantityOverflow`.

**Trừ quá mức (underflow) khi xóa / giảm.** `remove` giảm cả `count` và
`total_qty`, còn `reduce_qty` giảm `remaining_qty` và `total_qty`; cả hai đều
dùng `.checked_sub(..).expect(..)`. Nếu các bộ đếm không nhất quán,
chương trình panic ngay lập tức với một thông báo nêu tên bất biến bị phá vỡ,
thay vì âm thầm để một bộ đếm bị "quay vòng" (wrap) thành một con số khổng lồ.

**Giải phóng hai lần (double free).** `OrderPool::free` dùng `assert!` để kiểm tra cờ
`in_use` của ô. Nếu không có kiểm tra này, giải phóng cùng một ô hai lần sẽ đẩy
nó vào free stack hai lần, và sau đó hai lệnh khác nhau có thể nhận cùng một ô,
âm thầm làm hỏng hàng đợi của cả hai. Bộ khớp lệnh không bao giờ làm vậy; kiểm tra này
giúp phát hiện lỗi ngay tại lời gọi sai khi bổ sung mã mới, chẳng hạn trong
bài tập, thay vì để lỗi xuất hiện ở một thao tác về sau.

> [!NOTE]
> **Vì sao dùng `assert!` mà không phải `debug_assert!`?** `debug_assert!` bị
> loại khỏi bản dựng release, nơi các lỗi này cũng cần được phát hiện. Bản
> dựng release cũng tắt kiểm tra tràn số học mặc định, nên câu lệnh
> `count -= 1` có thể âm thầm quay vòng giá trị. Mỗi kiểm tra chỉ tốn một nhánh rẽ dễ dự
> đoán trên dữ liệu vốn đã nằm trong cache. Cấu hình release của crate đặt
> `panic = "abort"`, khiến tiến trình chấm dứt ngay khi panic trong tệp thực thi được
> biên dịch theo cấu hình đó (xem
> [chương 10](10-testing-and-performance.md#105-các-cấu-hình-biên-dịch-build-profile) về lưu ý đối với
> thư viện). Đó là chủ ý: với một hệ thống tài chính, dừng ngay và báo lỗi rõ ràng còn hơn tiếp
> tục chạy với một sổ lệnh đã hỏng.

## 3.8 Theo dõi ví dụ xuyên suốt

Ta bắt đầu với một bộ khớp lệnh rỗng có dung lượng 16 lệnh rồi phát lại ba lệnh đầu tiên của
ví dụ minh họa. Mỗi lời gọi còn nhận sink sự kiện làm tham số cuối cùng
(`&mut events`); tham số này được lược khỏi bảng để các dòng gọn hơn.

| Bước | Lời gọi | Tác động lên `pool` | Trạng thái hàng đợi |
|---|---|---|---|
| 1 | `process_order(1, Sell, 102, 5)` | `allocate` -> ô 0 | `ask[102]`: ô 0 |
| 2 | `process_order(2, Sell, 101, 3)` | `allocate` -> ô 1 | `ask[101]`: ô 1 |
| 3 | `process_order(3, Sell, 101, 4)` | `allocate` -> ô 2; `push_back` nối `1 -> 2` | `ask[101]`: ô 1, ô 2 (`total_qty = 7`) |
| 4 | `cancel_order(2)` *(giả định)* | `remove(slot 1)`: head trở thành ô 2; `free(1)` | `ask[101]`: chỉ còn ô 2 (`total_qty = 4`) |
| 5 | `process_order(7, Sell, 100, 1)` *(ID mới; `#4` đã dùng ở chương 1)* | `allocate` lấy ra ô 1, ô **được giải phóng gần nhất** | `ask[100]`: ô 1 |

Bước 5 cho thấy việc dùng lại theo LIFO (vào sau, ra trước): ô 1 được giải phóng
ở bước 4 và là ô đầu tiên được cấp lại.

## 3.9 Tóm tắt

- **Arena**: các lệnh nằm trong một mảng cố định và được truy cập bằng chỉ số `u32`.
- **Free stack**: allocate và free đều O(1), ô được giải phóng gần nhất được
  dùng lại trước.
- **Danh sách liên kết đôi intrusive**: các liên kết nằm ngay trong lệnh, nên việc
  thêm vào cuối, lấy từ đầu và xóa ở giữa đều có độ phức tạp O(1), không cấp phát.
- **`PriceLevel`** lưu sẵn `count` và `total_qty`, nên truy vấn độ sâu và kiểm
  tra "mức giá này có rỗng không?" đều O(1).

## 3.10 Tự thực hành

Hãy chạy các kiểm thử đơn vị của kho chứa lệnh, bao gồm kiểm thử xác nhận lỗi giải phóng hai lần được
phát hiện:

```sh
cargo test pool::
```

Sau đó hãy tự kiểm tra bố cục ở mục 3.3. Thêm dòng này vào cuối `main` trong
[`examples/walkthrough.rs`](../../examples/walkthrough.rs) rồi chạy ví dụ:

```rust,ignore
println!("Order is {} bytes", std::mem::size_of::<matching_engine::pool::Order>());
```

Xóa `#[repr(C)]` khỏi `Order` trong [`src/pool.rs`](../../src/pool.rs), chạy lại,
rồi so sánh với mục 3.3. (Nhớ hoàn tác thay đổi sau đó.)

**Tiếp theo:** [4. Bitset cho thang giá](04-price-ladder-bitset.md)
