# 4. Bitset cho thang giá (nhiều tầng)

> *Bản gốc tiếng Anh: [04-price-ladder-bitset.md](../04-price-ladder-bitset.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Nguồn: [`src/bitset.rs`](../../src/bitset.rs)

Module này liên tục trả lời một câu hỏi, và mỗi lần đều chỉ tốn O(1):

> **"Hiện giờ, mức giá tốt nhất có ít nhất một lệnh là mức nào?"**

- Với bid, giá tốt nhất là tick **cao nhất** còn có lệnh.
- Với ask, giá tốt nhất là tick **thấp nhất** còn có lệnh.

## 4.1 Bài toán

Vòng lặp khớp lệnh cần biết giá tốt nhất của phía đối diện ở mọi bước. Có vài
cách làm:

| Cách làm | Chi phí để tìm giá tốt nhất | Ghi chú |
|---|---|---|
| Duyệt `levels[0..ticks]` để tìm mức không rỗng đầu tiên | tối đa mỗi tick một bước | Đơn giản, nhưng quá chậm và khó đoán trước. |
| Cây có thứ tự (`BTreeMap`) | O(log n) | Phải lần theo con trỏ; có cấp phát khi chèn/xóa. |
| Lưu sẵn giá trị "tốt nhất", tính lại khi mức đó hết lệnh | thường O(1), đôi khi O(n) | Những lần quét lại thỉnh thoảng này chính là đợt tăng vọt độ trễ mà ta muốn tránh. |
| **Bitset kèm quét bit** | **O(1), luôn luôn** | Mỗi tick một bit; CPU tìm một bit bật chỉ bằng một chỉ thị (instruction). |

## 4.2 Ý tưởng 1: mỗi tick một bit

Ta giữ một bit cho mỗi tick: `1` = "có ít nhất một lệnh đang nằm chờ ở đây",
`0` = trống. Chương này trước hết đi theo một engine với 4096 tick mặc định (4.2
đến 4.5), rồi cho thấy cùng ý tưởng đó mở rộng ra số tick bất kỳ như thế nào
(4.6). 4096 tick cần 4096 bit = 64 word, mỗi word 64 bit.

```text
 words[0]  : tick     0 ..  63      0000...0000
 words[1]  : tick    64 .. 127      0000...0000
   ...
 words[63] : tick  4032 .. 4095     0000...0000
```

Để tìm **bit bật** (set bit, tức bit có giá trị 1) thấp nhất, ta dùng một chỉ
thị CPU: **đếm số bit 0 ở cuối** (count trailing zeros). Để tìm bit bật cao
nhất: **đếm số bit 0 ở đầu** (count leading zeros). Trên CPU hiện đại, cả hai đều
chỉ là một chỉ thị, nhưng chỉ làm việc trên *một* word 64 bit. Duyệt cả 64 word
thì vẫn tốn tới 64 bước. Ý tưởng 2 sẽ loại bỏ chỗ này.

### Quét bit trong 30 giây

Ta dùng số 8 bit cho ngắn:

```text
   value = 0b0010_1000
               ^  ^
               |  +-- bit bật thấp nhất  = bit 3
               +----- bit bật cao nhất   = bit 5

   trailing_zeros(value) = 3            -> chỉ số của bit bật THẤP NHẤT
   leading_zeros(value)  = 2            -> chỉ số của bit bật CAO NHẤT = (8 - 1) - 2 = 5
```

Với `u64`, bit bật cao nhất nằm ở vị trí `63 - leading_zeros`. Trong Rust, hai
phép này là các method `u64::trailing_zeros` và `u64::leading_zeros`. Trình
biên dịch dịch chúng thành một hoặc hai chỉ thị (ví dụ `tzcnt`/`bsf` và
`lzcnt`/`bsr` trên x86-64, `clz` hoặc `rbit` + `clz` trên ARM). Chỉ thị cụ thể
phụ thuộc vào nền tảng đích lúc biên dịch; build với `-C target-cpu=native` sẽ
cho phép trình biên dịch dùng các chỉ thị mới nhất.

## 4.3 Ý tưởng 2: thêm một word tóm tắt ở trên

Ta thêm một **tầng thứ hai**: một word 64 bit `summary` (tóm tắt), trong đó
**bit `g` có nghĩa là "word `g` không phải toàn số 0"**.

```text
                         summary (u64): mỗi word một bit
        bit:  63 62 ...      31 ...            3  2  1  0
             +--+--+-----+----+-----+---------+--+--+--+--+
             | 0| 0| ... | 1  | ... |         | 0| 0| 1| 0|
             +--+--+-----+-|--+-----+---------+--+--+-|+--+
                           |                          |
            +--------------+                          +-----+
            v                                               v
   words[31]  tick 1984..2047                      words[1]  tick 64..127
   +-----------------------------+                 +----------------------------+
   | ... bit 16 bật ...          |                 | ... bit 36 và 37 bật ...   |
   +-----------------------------+                 +----------------------------+
        = tick 2000                                     = tick 100 và 101
```

Với 4096 tick, dữ liệu chỉ đơn giản là:

```rust
summary: u64,           // level 2: bit g set  <=>  words[g] != 0
words: [u64; 64],       // level 1: bit b of words[g] set <=> tick g*64+b is active
```

Kích thước: `8 + 64 × 8 = 520 bytes`. Cả cấu trúc chỉ trải trên 9 đến 10 cache
line, và trên thực tế mỗi thao tác chỉ chạm tới word summary cộng với một hoặc
hai word khác. (`TickBitset` thật giữ 65 word này trong một mảng duy nhất, để
cùng một đoạn code cũng chạy được với nhiều tầng hơn; 4.6 sẽ cho thấy cách làm.)

### Đổi tick thành vị trí (word, bit)

Vì `64 = 2^6`, phép chia và phép lấy dư biến thành một phép dịch bit và một phép
mask:

```rust
let word_idx = tick >> 6;    // tick / 64
let bit_idx  = tick & 63;    // tick % 64
```

Ví dụ với `tick = 1234`: `1234 >> 6 = 19` (vì 19 × 64 = 1216) và
`1234 & 63 = 18`. Vậy tick 1234 là **bit 18 của word 19**. Kiểm tra lại:
19 × 64 + 18 = 1234. ✓

## 4.4 Các thao tác

> Các đoạn code trích trong phần này viết tường minh trường hợp hai tầng, để từng
> bước đều hiện rõ. [`src/bitset.rs`](../../src/bitset.rs) chạy đúng các bước đó
> trong một vòng lặp ngắn qua các tầng (4.6); với 4096 tick, tức là có hai tầng,
> vòng lặp làm đúng y như những gì trình bày ở đây.

### `set(tick)`: đánh dấu một tick có lệnh

```rust
self.words[word_idx] |= 1u64 << bit_idx;   // level 1: set the tick's bit
self.summary         |= 1u64 << word_idx;  // level 2: mark its word non-empty
```

Bật một bit đã bật thì không có gì thay đổi, nên `set` có tính **idempotent**
(gọi nhiều lần cũng như gọi một lần). Engine gọi nó mỗi khi có một lệnh nằm chờ
trong sổ, kể cả khi mức giá đó đã có lệnh từ trước. Mã nguồn thậm chí còn dừng
sớm: nếu word đó đã có bit bật từ trước thì bit summary cũng đã được bật, nên
không còn gì phải làm nữa.

### `lowest()`: ask tốt nhất

```rust
let word_idx = self.summary.trailing_zeros() as usize;   // step 1: first non-empty word
let bit_idx  = self.words[word_idx].trailing_zeros() as usize; // step 2: first set bit in it
Some(word_idx * 64 + bit_idx)
```

### `highest()`: bid tốt nhất

```rust
let word_idx = 63 - self.summary.leading_zeros() as usize;       // last non-empty word
let bit_idx  = 63 - self.words[word_idx].leading_zeros() as usize; // last set bit in it
Some(word_idx * 64 + bit_idx)
```

Cả hai hàm đều trả về `None` ngay nếu `summary == 0`. **Hai lần quét bit**, bất
kể sổ có 2 mức giá hay 4000.

### `clear(tick)`: đánh dấu một tick đã trống

```rust
self.words[word_idx] &= !(1u64 << bit_idx);      // clear the tick's bit
if self.words[word_idx] == 0 {                   // ...only if the word is now all zeros...
    self.summary &= !(1u64 << word_idx);         // ...clear the summary bit too
}
```

Câu `if` này rất quan trọng. Word 1 có thể chứa cả tick 100 **và** tick 101. Khi
xóa tick 100, ta **không** được xóa bit summary. Nếu xóa, summary sẽ báo word 1
rỗng trong khi tick 101 vẫn còn lệnh (một "âm tính giả", false negative), và
engine sẽ không bao giờ nhìn thấy lệnh ở 101.

### `test(tick)`: tick có lệnh không?

Chỉ đọc đúng một bit. `depth_snapshot` dùng nó để đi qua các mức giá (xem 4.7).

## 4.5 Ví dụ minh họa

Giả sử ta bật các tick **100, 101 và 2000**.

```text
 tick 100  -> word 1  (100 >> 6 = 1),  bit 36 (100 - 64)
 tick 101  -> word 1,                  bit 37
 tick 2000 -> word 31 (2000 >> 6 = 31, vì 31*64 = 1984), bit 16 (2000 - 1984)

 words[1]  = bit {36, 37}
 words[31] = bit {16}
 summary   = bit {1, 31}
```

**`lowest()`**

1. `summary.trailing_zeros()` = **1** → word không rỗng đầu tiên là `words[1]`.
2. `words[1].trailing_zeros()` = **36**.
3. Kết quả: `1 × 64 + 36 = 100`. ✓

**`highest()`**

1. `summary.leading_zeros()` = 32, nên chỉ số word là `63 − 32` = **31**.
2. `words[31].leading_zeros()` = 47, nên chỉ số bit là `63 − 47` = **16**.
3. Kết quả: `31 × 64 + 16 = 2000`. ✓

**`clear(100)`:** `words[1]` thành `{37}`, vẫn khác 0, nên summary giữ nguyên.
Lúc này `lowest()` trả về 101.

**`clear(101)`:** `words[1]` thành `0`, nên bit 1 của summary cũng bị xóa.
Summary giờ là `{31}`, và `lowest()` trả về 2000.

## 4.6 Vì sao là 4096 tick, và làm sao để có nhiều hơn

`4096 = 64 × 64`. Một word summary có đúng 64 bit, mỗi bit ứng với một word, nên
hai tầng phủ được 64 × 64 tick. Đó là lý do 4096 là giá trị mặc định
(`DEFAULT_TICKS`), và trong một thời gian dài đó cũng là kích thước duy nhất:
bitset từng là một struct hai tầng cố định, và một phép kiểm tra lúc biên dịch
ngăn không cho ai làm nó lớn hơn.

Các mã giao dịch (instrument) thật cần nhiều hơn thế. Một cổ phiếu có giá quanh
100.00 với bước giá (tick size) 0.01 đã cần 10,000 tick chỉ để lên tới 100.00.
Lối thoát chính là con đường mà word tóm tắt đã đi: **thêm một tầng ở trên mỗi
khi tầng bên dưới có nhiều hơn một word.**

| Số tick (tối đa) | Số tầng | Số word mỗi tầng, từ dưới lên trên | Số lần quét bit mỗi lần tra cứu |
|---|---|---|---|
| 64 | 1 | 1 | 1 |
| 4096 (`64²`) | 2 | 64, 1 | 2 |
| 262,144 (`64³`) | 3 | 4096, 64, 1 | 3 |
| 16,777,216 (`64⁴`, giới hạn) | 4 | 262,144, 4096, 64, 1 | 4 |

`Engine::with_ticks(capacity, ticks)` chọn số tick; `TickBitset::new(ticks)`
tính xem số đó cần bao nhiêu tầng và lưu toàn bộ word của các tầng trong **một
mảng phẳng duy nhất**, tầng dưới cùng trước, với vị trí bắt đầu của mỗi tầng nằm
trong `offsets`. Chỉ một lần cấp phát, lúc khởi tạo, giống như mọi thứ khác.

Khi đó mỗi thao tác trở thành một vòng lặp ngắn qua các tầng. `lowest()` đi
xuống từ word duy nhất ở tầng trên cùng, mỗi lần quét bit chọn ra word cần đọc ở
tầng bên dưới:

```rust
let mut index = 0;
for level in (0..self.levels).rev() {                 // top level first
    let word = self.words[self.offsets[level] + index];
    index = index * 64 + word.trailing_zeros() as usize;
}
Some(index)                                           // a tick, on level 0
```

`set(tick)` đi lên, và dừng ở word đầu tiên vốn đã không rỗng từ trước (bit
summary của nó, và mọi bit phía trên, chắc chắn đã được bật rồi):

```rust
let mut index = tick;
for level in 0..self.levels {                         // bottom level first
    let slot = self.offsets[level] + (index >> 6);
    let was_empty = self.words[slot] == 0;
    self.words[slot] |= 1u64 << (index & 63);
    if !was_empty { return; }
    index >>= 6;                                      // this word's bit, one level up
}
```

`clear(tick)` làm ngược lại và dừng ở word đầu tiên *vẫn còn* không rỗng: đúng
quy tắc ở 4.4, áp dụng cho mọi tầng. `highest()` chính là `lowest()` nhưng dùng
`63 - leading_zeros`.

Với hai tầng, các vòng lặp chạy đúng những bước ở 4.4, và các benchmark
(chương 10) không cho thấy chi phí nào đo được cho tính tổng quát này. Một thang
giá rộng hơn tốn thêm một lần quét bit cho mỗi tầng tăng thêm, vẫn là O(1), cùng
với bộ nhớ: mỗi tick cần hai mức giá, mỗi mức 24 byte, nên 4096 tick tốn 196 KB
và 262,144 tick tốn 12.6 MB. Chính bộ nhớ đó, hơn là các lần quét bit, mới là lý
do thật sự để chọn số tick riêng cho từng mã giao dịch thay vì làm sổ lệnh nào
cũng thật lớn.

Giới hạn kích thước giờ được kiểm tra lúc tạo bitset, chứ không phải lúc biên
dịch chương trình, vì phải đến lúc đó mới biết kích thước: `TickBitset::new`
panic khi số tick bằng 0 hoặc lớn hơn `MAX_LADDER_TICKS` (2^24, bốn tầng).

## 4.7 Engine dùng nó như thế nào

`Engine` sở hữu **hai** bitset, mỗi phía một cái:

| Bitset | Bit được bật khi... | Giá tốt nhất là... | Truy vấn |
|---|---|---|---|
| `bid_bitset` | một bid nằm chờ ở tick đó | tick được bật **cao nhất** | `highest()` (`Engine::best_bid`) |
| `ask_bitset` | một ask nằm chờ ở tick đó | tick được bật **thấp nhất** | `lowest()` (`Engine::best_ask`) |

Bit được cập nhật ở đúng ba chỗ:

1. **Lệnh nằm chờ trong sổ** (`rest`, được `submit_order` và `restore` dùng,
   sau khi `push_back` thành công): `set(tick)`.
2. **Mức giá bị khớp cạn** (`match_incoming`, khi `level.is_empty()`, dù lệnh
   maker cuối cùng đã được khớp hết hay bị gỡ bởi cơ chế chống tự giao dịch
   (self-trade prevention)): `clear(tick)`.
3. **Lệnh cuối cùng ở một mức giá bị hủy** (`remove_cancelled`, được dùng bởi
   `cancel_order`, việc giảm số lượng về 0 và các lệnh hủy hàng loạt):
   `clear(tick)`.

**Bất biến** (invariant): bit `t` được bật ⇔ `levels[t]` không rỗng. Bitset là
một *chỉ mục* (index) đặt trên mảng các mức giá: một cách nhanh để tìm các phần
tử không rỗng.

> [!NOTE]
> `Engine::depth_snapshot` cần mức không rỗng *kế tiếp* sau mức tốt nhất. Code
> hiện tại làm việc đó bằng cách gọi `test(t)` lần lượt trên từng tick
> (`next_lower_active` / `next_higher_active`), tức là một lần quét tuyến tính,
> dù bitset có thể trả lời nhanh hơn: che đi (mask) các bit từ `t` trở xuống
> (với ask, vì ask quét lên) hoặc từ `t` trở lên (với bid, vì bid quét xuống),
> rồi quét bit phần còn lại. Đây là một tối ưu dễ làm và có nhiều điều để học
> (xem phần bài tập). Nó không nằm trên hot path xử lý lệnh.

## 4.8 Tóm tắt

- Mỗi tick một bit, gom thành các word 64 bit.
- Một word summary ghi lại word nào không rỗng; với hơn 4096 tick thì có thêm
  các tầng tóm tắt, cho tới khi chỉ còn một word duy nhất ở trên cùng.
- Giá tốt nhất = mỗi tầng một lần quét bit bằng phần cứng (hai lần với 4096
  tick): **O(1)**, không cấp phát.
- `clear` chỉ xóa một bit summary khi cả word bên dưới nó đã rỗng.

## 4.9 Tự thực hành

Chạy các unit test của bitset:

```sh
cargo test bitset::
```

Sau đó hãy tự viết một test nhỏ ở cuối module `tests` trong
[`src/bitset.rs`](../../src/bitset.rs): trong một `TickBitset::new(4096)`, bật
các tick `5`, `70` và `4000`, đoán trước `lowest()` và `highest()` trả về gì và
những bit nào của summary đang bật, rồi kiểm tra bằng `assert_eq!`. Xóa tick `5`
rồi đoán lại. Sau đó làm tương tự với một `TickBitset::new(300_000)`, với các
tick `5`, `70` và `250_000`: `lowest()` đọc word nào ở mỗi tầng trong bốn tầng
của nó?

**Tiếp theo:** [5. Bảng băm phẳng (flat hash map)](05-hash-map.md)
