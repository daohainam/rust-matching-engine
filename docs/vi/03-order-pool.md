# 3. Order pool: kho chứa lệnh (arena, free list và hàng đợi FIFO)

> *Bản gốc tiếng Anh: [03-order-pool.md](../03-order-pool.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Mã nguồn: [`src/pool.rs`](../../src/pool.rs)

Module này trả lời hai câu hỏi:

1. **Các lệnh nằm ở đâu trong bộ nhớ?** Trong một mảng lớn được cấp phát sẵn
   (một *arena*).
2. **Làm sao giữ các lệnh cùng mức giá theo đúng thứ tự đến?** Bằng các danh
   sách liên kết mà các liên kết là chỉ số mảng (một danh sách liên kết
   *intrusive*).

## 3.1 Bài toán

Một mức giá cần một hàng đợi các lệnh: lệnh mới vào cuối hàng, việc khớp lệnh
lấy từ đầu hàng, còn hủy lệnh thì xóa ở *bất kỳ đâu*. Lời giải Rust hiển nhiên
là `VecDeque<Order>` hoặc `LinkedList<Order>`, nhưng:

- `VecDeque` xóa phần tử ở giữa mất O(n) (mọi phần tử phía sau phải dịch lên).
- `LinkedList` cấp phát một nút trên heap cho mỗi lệnh, và các nút cuối cùng nằm
  rải rác khắp bộ nhớ.
- Cả hai đều cấp phát trên hot path (đoạn code chạy cho mọi lệnh).

Ta muốn có: **thêm vào cuối O(1), lấy ra từ đầu O(1), xóa ở giữa O(1), không
cấp phát.**

## 3.2 Ý tưởng 1: arena

Một **arena** là một mảng lớn, cấp phát đúng một lần. Các đối tượng được gọi
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
- Các lệnh nằm **sát cạnh nhau** trong bộ nhớ, thứ mà prefetcher (bộ nạp trước)
  của CPU rất thích.
- Không `unsafe`, không lifetime: chỉ số chỉ là một con số, nên borrow checker
  (bộ kiểm tra mượn của trình biên dịch) của Rust không phải "cản đường" ta.
  (Một mạng nhện tự tham chiếu gồm các tham chiếu `&mut Order` thì không thể
  diễn đạt được trong Rust an toàn.)

Cái giá phải trả cho sự tự do đó: **trình biên dịch không còn kiểm tra chỉ số
có còn hợp lệ hay không.** Giả sử có đoạn code giữ chỉ số ô `7` cho lệnh `#42`,
rồi `#42` bị hủy và ô 7 được giao cho một lệnh mới `#99`. Chỉ số cũ vẫn "chạy
được": `pool.get(7)` vui vẻ trả về dữ liệu của `#99`, không báo lỗi gì. Đây là
phiên bản "chỉ số" của *con trỏ treo* (dangling pointer), thường gọi là **chỉ số
cũ** (stale index). Nó không thể gây ra hành vi không xác định (undefined
behavior, UB), vì ô nhớ luôn chứa một `Order` hợp lệ, nhưng nó có thể âm thầm
đọc hoặc sửa nhầm lệnh.

Engine tránh lỗi này bằng kỷ luật: nó không bao giờ giữ một chỉ số qua nhiều lời
gọi. Những tham chiếu sống lâu duy nhất tới một ô là mục trong bảng băm và các
liên kết của hàng đợi, và cả hai đều bị xóa ngay trong cùng bước giải phóng ô.
Chỉ có `free` kiểm tra cờ `in_use` của ô (mục 3.7); `get` thì không. Nếu cần an
toàn hơn, cách sửa phổ biến là dùng **chỉ số có thế hệ** (generational index):
lưu trong mỗi ô một bộ đếm tăng lên mỗi lần ô được dùng lại, và đưa luôn bộ đếm
đó vào chỉ số, để một chỉ số cũ không còn khớp nữa.

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
}
```

`#[repr(C)]` giữ các trường theo đúng thứ tự khai báo. Trình biên dịch chèn thêm
*padding* (byte đệm) để mỗi trường được căn lề (aligned). Đo bằng `offset_of!`
ta được:

```text
 byte  0         8        16        24   28   32        36        40   41     48
       +---------+---------+---------+----+----+--------+--------+----+------+
       |order_id |remaining|sequence |tick|side|next_idx|prev_idx|in_ |      |
       |  u64    | _qty u64|  u64    |u32 | u8 |  u32   |  u32   |use |      |
       +---------+---------+---------+----+----+--------+--------+----+------+
                                            ^^^ 3 byte padding              ^^^ 7 byte padding
       tổng size_of::<Order>() = 48 byte
```

Hai khoảng trống đó từ đâu ra? Mỗi kiểu dữ liệu có một **yêu cầu căn lề**
(alignment): `u32` phải bắt đầu ở địa chỉ chia hết cho 4, `u64` ở địa chỉ chia
hết cho 8.

- **3 byte sau `side`.** `side` chiếm một byte, ở offset 28, nên byte trống kế
  tiếp là 29. `next_idx` là một `u32` và phải bắt đầu ở địa chỉ chia hết cho 4,
  nên trình biên dịch bỏ qua các byte từ 29 đến 31 và đặt nó ở 32.
- **7 byte ở cuối.** Các trường kết thúc ở byte 41 (`in_use` nằm ở 40). Nhưng
  `Order` chứa các `u64`, nên cả struct phải được căn lề 8 byte, và kích thước
  của nó được làm tròn lên bội số của 8: 48. Nhờ vậy, trong một mảng các lệnh,
  mọi phần tử (và mọi `u64` bên trong nó) cũng đều bắt đầu ở địa chỉ được căn
  lề đúng.

Vậy tại sao lại đòi dùng `repr(C)`? Nếu không có nó, Rust dùng bố cục mặc định
và được tự do **sắp xếp lại thứ tự các trường** để giảm padding; với struct này,
nó tự chọn một bố cục 40 byte. `repr(C)` bỏ đi 8 byte đó để đổi lấy một bố cục
cố định và dễ đoán: hình vẽ phía trên được bảo đảm, chứ không phải một chi tiết
của trình biên dịch có thể thay đổi, nhờ vậy ta vẽ ra được, đo được và dạy
được. Cả hai lựa chọn đều hợp lý; các thí nghiệm trong ghi chú bên dưới sẽ so
sánh chúng.

> [!NOTE]
> Chú thích trong mã nguồn nói rằng lệnh có kích thước "sized to fit within a
> single 64-byte cache line" (vừa trong một cache line 64 byte). Điều đó đúng
> theo nghĩa 48 ≤ 64, nhưng 48 không chia hết 64, nên trong một mảng xếp sát
> nhau, nhiều lệnh sẽ **nằm vắt ngang hai cache line** (nếu mảng bắt đầu đúng ở
> ranh giới một line, lệnh số 1 chiếm các byte 48 đến 95, vượt qua ranh giới ở
> byte 64). [Chương 11](11-limitations-and-exercises.md) có liệt kê hai thí
> nghiệm dễ làm: sắp xếp lại các trường (hoặc bỏ `repr(C)` và để trình biên dịch
> tự làm) để thu bản ghi xuống 40 byte, hoặc căn lề nó theo 64 byte để mỗi lệnh
> sở hữu đúng một cache line.

## 3.4 Ý tưởng 2: free list (ngăn xếp các ô trống)

Làm sao tìm một ô chưa dùng trong O(1)? Giữ một **ngăn xếp các chỉ số trống**:

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
trả về ô `0`, lần kế tiếp trả về ô `1`, và cứ thế. (Thuần túy để cho đẹp, như chú
thích trong mã nguồn đã nói.)

Vì đây là một *ngăn xếp* (vào sau, ra trước, LIFO), ô được giải phóng gần nhất sẽ
được dùng lại đầu tiên. Vùng nhớ ở ô đó vừa được truy cập, nên nhiều khả năng
nó vẫn còn trong cache: một lợi thế nhỏ mà không tốn thêm gì.

`live_count()` đơn giản là `capacity - free_top`.

### Khi cấp phát thất bại

Nếu `free_top == 0` thì arena đã đầy và `allocate` trả về `None`. Engine biến
điều đó thành một sự kiện `OrderRejected` với lý do `BookFull` (chỉ áp dụng cho
phần còn lại của lệnh cần nằm chờ trong sổ; một lệnh khớp hết thì không cần ô
nào).

## 3.5 Ý tưởng 3: danh sách liên kết đôi intrusive

"Intrusive" nghĩa là các liên kết của danh sách được lưu **ngay bên trong phần
tử** (`next_idx`, `prev_idx` trong `Order`) thay vì trong một đối tượng nút
riêng. Nói cách khác, chính lệnh *là* nút của danh sách: không cần cấp phát thêm.

Mỗi tick giá có một bộ mô tả rất nhỏ, `PriceLevel`:

```rust
pub struct PriceLevel {
    pub head: u32,       // oldest order (first to trade), or NIL if empty
    pub tail: u32,       // newest order (last to trade), or NIL if empty
    pub count: u32,      // number of orders at this level
    pub total_qty: u64,  // sum of remaining_qty of all orders (O(1) depth queries)
}
```

Hàng đợi của tick ask 101 trong ví dụ xuyên suốt (`#2` rồi đến `#3`, nằm ở các ô
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
giá trị cộng với một thẻ đánh dấu kèm padding để căn lề), tức là làm kích thước
của cả hai liên kết tăng gấp đôi. Đó cũng là lý do `OrderPool::new` yêu cầu
`capacity < u32::MAX`: giá trị `u32::MAX` được giữ riêng.

### Thao tác 1: `push_back` (một lệnh mới vào cuối hàng đợi)

Thêm ô `idx` vào cuối hàng (tail). Có hai trường hợp:

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

Sổ sách đi kèm: `count += 1` và `total_qty += X.remaining_qty`. Phép cộng này là
một `checked_add` (xem 3.7).

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

Đoạn code dưới đây làm đúng như vậy:

```rust
if prev != NIL { pool.get_mut(prev).next_idx = next; } else { self.head = next; }
if next != NIL { pool.get_mut(next).prev_idx = prev; } else { self.tail = prev; }
```

Sau đó, các liên kết của chính nút bị xóa được đặt lại thành `NIL`, còn `count`
và `total_qty` bị giảm đi (bằng phép trừ có kiểm tra, xem 3.7).

Đây chính là lý do danh sách phải liên kết **đôi**: để hủy `B` trong O(1), ta
phải với tới được `A` (để cập nhật `A.next`) mà không cần đi từ đầu danh sách.

### Thao tác 3: `reduce_qty` (khớp một phần)

Khi một maker được khớp một phần, nó vẫn ở đầu hàng. Chỉ có các con số thay đổi:

```text
   order.remaining_qty -= delta
   level.total_qty     -= delta
```

Thứ tự trong hàng đợi (và do đó cả ưu tiên về thời gian của maker) không hề bị
đụng tới. Một lệnh đã khớp một phần **giữ nguyên vị trí** ở đầu hàng.

## 3.6 Các bất biến (những điều luôn phải đúng)

Đây là các quy tắc mà phần còn lại của engine dựa vào. Nếu một quy tắc bị phá
vỡ, sổ lệnh đã bị hỏng.

| # | Bất biến | Được duy trì bởi |
|---|---|---|
| 1 | `level.head == NIL` ⇔ `level.tail == NIL` ⇔ `level.count == 0` ⇔ `level.total_qty == 0` | `push_back`, `remove` |
| 2 | `level.total_qty` = tổng `remaining_qty` của mọi lệnh trong hàng đợi. (Vế "`total_qty == 0` ⇔ rỗng" của bất biến 1 còn dựa vào việc engine không bao giờ để lệnh có số lượng bằng 0 nằm chờ trong sổ: `process_order` từ chối `qty == 0`; bản thân `PriceLevel` không kiểm tra.) | `push_back` (+), `remove` (−), `reduce_qty` (−) |
| 3 | Đi theo `head -> next_idx -> ...` sẽ thăm đúng `count` lệnh và kết thúc ở `tail` | `push_back`, `remove` |
| 4 | `a.next_idx == b` ⇔ `b.prev_idx == a` | `push_back`, `remove` |
| 5 | Một ô nằm trong free stack ⇔ `in_use == false` | `allocate`, `free` |
| 6 | Mọi lệnh đang sống đều nằm trong đúng một hàng đợi **và** có một mục trong `id_map` | `Engine` |

## 3.7 Các kiểm tra phòng thủ

Pool tự bảo vệ mình trước ba loại rắc rối, tất cả đều thấy rõ trong code:

**Tràn số khi chèn (overflow).** `push_back` dùng `checked_add` cho `total_qty`.
Nếu một client có ý xấu hoặc bị lỗi gửi lên số lượng lớn đến mức tổng của mức
giá vượt quá `u64::MAX`, `push_back` trả về `false` *mà không đụng vào bất cứ
thứ gì*. Phương thức này được đánh dấu `#[must_use]`, và engine phản ứng bằng
cách xóa mục `id_map` của lệnh, giải phóng ô, rồi phát sự kiện `OrderRejected`
với lý do `LevelQuantityOverflow`.

**Trừ quá mức (underflow) khi xóa / giảm.** `remove` giảm cả `count` và
`total_qty`, còn `reduce_qty` giảm `remaining_qty` và `total_qty`, tất cả đều
dùng `.checked_sub(..).expect(..)`. Nếu sổ sách có lúc nào đó không nhất quán,
chương trình panic ngay lập tức với một thông báo nêu tên bất biến bị phá vỡ,
thay vì âm thầm để một bộ đếm bị "quay vòng" (wrap) thành một con số khổng lồ.

**Free hai lần (double free).** `OrderPool::free` kiểm tra cờ `in_use` của ô
bằng một `assert!` thật sự. Nếu không, việc giải phóng cùng một ô hai lần sẽ đẩy
nó vào free stack hai lần, và sau đó hai lệnh khác nhau có thể nhận cùng một ô,
âm thầm làm hỏng hàng đợi của cả hai. Engine không bao giờ làm vậy; kiểm tra này
có mặt để một lỗi trong code mới (chẳng hạn một bài tập) sẽ thất bại ngay tại
lời gọi sai, thay vì mãi về sau mới lộ ra.

> [!NOTE]
> **Vì sao dùng `assert!` mà không phải `debug_assert!`?** `debug_assert!` bị
> loại khỏi bản build release, mà đó lại chính là nơi không ai để mắt tới. Bản
> build release cũng thôi kiểm tra tràn số học, nên một câu lệnh trần
> `count -= 1` sẽ âm thầm bị wrap ở đó. Mỗi kiểm tra này chỉ tốn một nhánh rẽ
> dễ dự đoán, trên dữ liệu vốn đã nằm trong cache. Profile release của crate đặt
> `panic = "abort"`, biến một panic như vậy thành việc dừng hẳn tiến trình
> (process abort) trong một binary được build với cấu hình đó (xem
> [chương 10](10-testing-and-performance.md#105-các-build-profile) về lưu ý đối với
> thư viện). Đó là chủ ý: với một hệ thống tài chính, dừng lại ầm ĩ còn hơn tiếp
> tục chạy với một sổ lệnh đã hỏng.

## 3.8 Đi qua một lượt với ví dụ xuyên suốt

Bắt đầu từ một engine rỗng với sức chứa 16 và phát lại ba lệnh đầu tiên của ví
dụ minh họa. Mỗi lời gọi còn nhận sink sự kiện làm tham số cuối cùng
(`&mut events`); ta lược nó khỏi bảng cho các dòng ngắn gọn.

| Bước | Lời gọi | Tác động lên `pool` | Trạng thái hàng đợi |
|---|---|---|---|
| 1 | `process_order(1, Sell, 102, 5)` | `allocate` -> ô 0 | `ask[102]`: ô 0 |
| 2 | `process_order(2, Sell, 101, 3)` | `allocate` -> ô 1 | `ask[101]`: ô 1 |
| 3 | `process_order(3, Sell, 101, 4)` | `allocate` -> ô 2; `push_back` nối `1 -> 2` | `ask[101]`: ô 1, ô 2 (`total_qty = 7`) |
| 4 | `cancel_order(2)` *(giả định)* | `remove(slot 1)`: head trở thành ô 2; `free(1)` | `ask[101]`: chỉ còn ô 2 (`total_qty = 4`) |
| 5 | `process_order(7, Sell, 100, 1)` *(id mới; `#4` đã dùng ở chương 1)* | `allocate` lấy ra ô 1, ô **được giải phóng gần nhất** | `ask[100]`: ô 1 |

Bước 5 cho thấy việc dùng lại theo LIFO (vào sau, ra trước): ô 1 được giải phóng
ở bước 4 và là ô đầu tiên được cấp lại.

## 3.9 Tóm tắt

- **Arena**: các lệnh nằm trong một mảng cố định và được gọi bằng chỉ số `u32`.
- **Free stack**: allocate và free đều O(1), ô được giải phóng gần nhất được
  dùng lại trước.
- **Danh sách liên kết đôi intrusive**: các liên kết nằm ngay trong lệnh, cho ra
  push-back, pop-front và remove-from-middle đều O(1) mà không cấp phát.
- **`PriceLevel`** lưu sẵn `count` và `total_qty`, nên truy vấn độ sâu và kiểm
  tra "mức giá này có rỗng không?" đều O(1).

## 3.10 Tự thực hành

Hãy chạy các unit test của pool, trong đó có test cho thấy lỗi free hai lần bị
bắt lại:

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
