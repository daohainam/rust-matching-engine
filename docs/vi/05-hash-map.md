# 5. Bảng băm phẳng (flat hash map): `OrderId → Arena Slot`

> *Bản gốc tiếng Anh: [05-hash-map.md](../05-hash-map.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Nguồn: [`src/hashmap.rs`](../../src/hashmap.rs)

## 5.1 Vì sao ta cần nó?

Khi khớp lệnh, engine không cần bảng này để *tìm* lệnh: nó lấy lệnh trực tiếp từ
đầu hàng đợi của mức giá (chỉ sau đó mới xóa từng maker đã khớp hết khỏi bảng).
**Hủy lệnh thì cần đến bảng này.** Client có thể gửi yêu cầu *"hủy lệnh 12345"*,
khi đó engine phải tìm lệnh 12345 trong arena mà không quét toàn bộ sổ lệnh.

```text
   cancel_order(12345)  ->  id_map.remove(12345)  ->  ô arena 987  ->  gỡ nó khỏi danh sách
```

Bảng này còn giúp **phát hiện lệnh trùng id**: trước khi nhận một lệnh, engine
kiểm tra `id_map.get(order_id).is_some()` và từ chối nếu id đó đang được dùng.

## 5.2 Vì sao không dùng `std::collections::HashMap`?

Đó là một bảng băm đa dụng rất tốt, nhưng với việc này thì có mấy điểm không hợp:

- Nó **cấp phát**, và **đổi kích thước** (cấp phát bảng mới rồi băm lại toàn bộ)
  khi lớn lên. Cả hai đều gây ra các đợt tăng vọt độ trễ.
- Hàm băm mặc định của nó (SipHash) được thiết kế để chống kẻ tấn công cố tình
  tạo ra các khóa va chạm nhau; việc đó tốn chu kỳ CPU mà ta không muốn bỏ ra cho
  mỗi lệnh. (Module này có được gần hết lớp bảo vệ đó với giá rẻ hơn nhiều, nhờ
  một seed bí mật: xem 5.7.)
- Ta biết trước số phần tử tối đa (sức chứa của pool), nên có thể định cỡ bảng
  **một lần** và không bao giờ phải đổi kích thước.

Vì vậy module này triển khai một bảng băm gọn nhất có thể, vừa đủ đáp ứng nhu cầu
của ta.

## 5.3 Địa chỉ mở với dò tuyến tính

Bảng băm kiểu *nối chuỗi riêng* (separate chaining) giữ một danh sách cho mỗi
bucket. Bảng **địa chỉ mở** (open addressing) lưu mọi phần tử **trực tiếp trong
một mảng phẳng**; nếu một ô (slot) đã bị chiếm, nó thử ô kế tiếp. Biến thể này
được gọi là **dò tuyến tính** (linear probing).

```rust
struct Slot { key: u64, value: u32 }       // 16 bytes (with padding)

pub struct FlatHashMap {
    slots: Box<[Slot]>,    // the whole table
    mask: usize,           // slots.len() - 1
    len: usize,
    seed: u64,             // secret, random per table (see 5.7)
}
```

Một ô trống có `key == u64::MAX` (gọi là `EMPTY` / `RESERVED_KEY`).
Nghĩa là `u64::MAX` không bao giờ được làm id của một lệnh thật (xem thêm ở 5.8).

### Chèn và tra cứu qua ví dụ

> Bốn khóa dưới đây là giả lập; "ô lý tưởng" (ideal slot) của chúng được chọn
> để cố tình *va chạm*. Hàm băm thật rải các khóa theo kiểu khác, nhưng cơ chế
> thì y hệt.

Bảng có 8 ô. Ta chèn `A`, `B`, `C`, `D`; ô lý tưởng của chúng (hash & mask) lần
lượt là `A→2`, `B→2`, `C→3`, `D→2`.

```text
 ô:         0    1    2    3    4    5    6    7
          [   ][   ][ A ][   ][   ][   ][   ][   ]     chèn A: ô lý tưởng 2 còn trống          -> ô 2
          [   ][   ][ A ][ B ][   ][   ][   ][   ]     chèn B: ô lý tưởng 2 đã bị chiếm, thử 3 -> ô 3
          [   ][   ][ A ][ B ][ C ][   ][   ][   ]     chèn C: ô lý tưởng 3 đã bị chiếm, thử 4 -> ô 4
          [   ][   ][ A ][ B ][ C ][ D ][   ][   ]     chèn D: ô 2, 3, 4 đã bị chiếm, thử 5    -> ô 5
```

**Tra cứu** lặp lại đúng hành trình đó: bắt đầu ở ô lý tưởng rồi lần lượt tiến
qua các ô kế tiếp cho đến khi tìm thấy khóa (**hit**) hoặc gặp một ô `EMPTY`
(**miss**: khóa không thể nằm xa hơn, vì nếu có thì nó đã được đặt vào ô trống
đó).

```text
 get(C): bắt đầu ở ô lý tưởng 3 -> B (không phải C) -> ô 4 -> C  TÌM THẤY    (2 lần dò)
 get(E) với ô lý tưởng 2:  A, B, C, D, rồi ô 6 là EMPTY  -> KHÔNG THẤY
```

Code chỉ là một vòng lặp gọn:

```rust
pub fn get(&self, key: u64) -> Option<u32> {
    let mut idx = self.hash(key) & self.mask;
    loop {
        let slot = &self.slots[idx];
        if slot.key == EMPTY { return None; }          // reached a hole: miss
        if slot.key == key   { return Some(slot.value); } // hit
        idx = (idx + 1) & self.mask;                   // next slot, wrapping around
    }
}
```

> [!WARNING]
> Vòng lặp này chỉ dừng được nếu bảng luôn có **ít nhất một ô `EMPTY`**. Một
> bảng đầy hoàn toàn sẽ khiến việc tra một khóa không tồn tại quay vòng mãi mãi.
> Cách định cỡ của engine chừa nhiều ô trống (5.5), và `insert` bắt buộc mức
> tối thiểu đó cho *bất kỳ ai* dùng bảng: nó panic thay vì lấp ô trống cuối
> cùng. Panic rõ ràng dễ gỡ lỗi hơn nhiều so với một vòng lặp treo mà không báo
> lỗi.

## 5.4 Hai mẹo: kích thước lũy thừa của hai và hàm băm tốt

### Mẹo mask

Để đổi một giá trị băm thành số thứ tự ô, bình thường bạn sẽ viết `hash % len`.
Phép chia thì chậm. Nếu `len` là **lũy thừa của hai** thì

```text
   hash % len  ==  hash & (len - 1)            ví dụ len = 32:  hash & 0b11111
```

chỉ còn là một phép AND rẻ tiền. Đó là lý do có `mask = capacity - 1`.
`FlatHashMap::with_capacity` làm tròn kích thước lên bằng `next_power_of_two()`.

### Hàm băm

Id lệnh thường theo những khuôn mẫu: bước nhảy đều (bội số của 1024), id mang số
hiệu gateway ở các bit cao... Nếu dùng thẳng `id & mask`, những khóa như vậy sẽ
bị dồn vào cùng vài ô. (Id tuần tự thuần túy `1, 2, 3, ...` tình cờ lại là
trường hợp tốt nhất của `id & mask`, nhưng bạn không thể trông cậy vào điều đó.)
Hàm dưới đây trộn các bit sao cho **mọi bit đầu vào đều ảnh hưởng tới mọi bit
đầu ra** (một *hiệu ứng tuyết lở*, avalanche):

```rust
fn hash(&self, key: u64) -> usize {
    let mut z = (key ^ self.seed).wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    (z ^ (z >> 31)) as usize
}
```

Ngoài phép XOR đầu tiên với `self.seed`, đây chính là finalizer của
**SplitMix64**. Cả hàm gồm mười phép toán đơn giản (bốn xor, một add, ba shift,
hai multiply), không có nhánh rẽ, và phân bố rất tốt. Các bit thấp, tức phần mà
mask giữ lại, cũng được trộn kỹ.

> Nó *không* an toàn về mặt mật mã. Chính seed bí mật mới là thứ ngăn kẻ tấn công
> đoán trước một id sẽ rơi vào ô nào; 5.7 sẽ cho thấy vì sao điều đó quan trọng.

## 5.5 Định cỡ: vì sao bảng không bao giờ đầy

```rust
let capacity = (capacity_hint.max(16) * 2).next_power_of_two();
```

Bảng có **số ô ít nhất gấp đôi số phần tử tối đa** (làm tròn lên thành lũy thừa
của hai). Vì vậy hệ số tải (load factor), tức số phần tử chia cho số ô, không
bao giờ vượt quá **50%**. Khi engine có sức chứa từ 16 lệnh trở lên, hệ số tải
vẫn trên 25% ngay cả khi sổ đầy hoàn toàn (nếu sổ mới đầy một phần thì hệ số
này còn thấp hơn):

| `order_capacity` | `slots` | hệ số tải khi đầy |
|---|---|---|
| 10,000 | 32,768 | 30.5% |
| 16,384 | 32,768 | 50% |
| 16,385 | 65,536 | 25% |

Số phần tử không bao giờ vượt quá `order_capacity`, vì engine chỉ chèn vào
`id_map` **sau khi** `pool.allocate` thành công, mà pool chứa tối đa
`order_capacity` lệnh. Vì vậy bảng luôn có nhiều ô trống.

Có hai cơ chế bảo vệ bảng ngay cả khi nó được dùng riêng lẻ:

- Phép tính kích thước dùng số học có kiểm tra (`checked_mul(2)`, rồi
  `checked_next_power_of_two()`). Giá trị gợi ý (hint) không hợp lệ sẽ gây panic
  với thông báo "hash map capacity overflows usize", thay vì tràn số rồi tạo ra
  một bảng quá nhỏ trong bản build release, nơi kiểm tra tràn số bị tắt.
- `insert` panic với thông báo "FlatHashMap is full" thay vì lấp ô trống cuối
  cùng, nên các vòng dò ở 5.3 luôn dừng được.

Vì sao phải để bảng thưa như vậy? Dò tuyến tính chậm đi nhanh chóng khi bảng đầy
dần. Dưới đây là ước lượng kinh điển (của Knuth) về số lần dò trung bình ở hệ
số tải `α`:

| Hệ số tải `α` | Hit (tìm thấy) | Miss (không thấy) |
|---|---|---|
| 25% | ≈ 1.2 | ≈ 1.4 |
| 50% | 1.5 | 2.5 |
| 90% | 5.5 | 50.5 |

Để trống một nửa là sự đánh đổi hợp lý giữa bộ nhớ và tốc độ: một lần hủy lệnh
thường chỉ cần một hoặc hai lần dò trong một hoặc hai cache line liền kề.

## 5.6 Xóa: phần khó

Chèn và tra cứu khá đơn giản. **Xóa** mới là thao tác khiến địa chỉ mở trở nên
phức tạp.

### Cách ngây thơ là sai

Xét bảng trong ví dụ trên. Nếu chỉ đánh dấu ô của `B` là trống:

```text
 ô:         0    1    2    3    4    5    6    7
          [   ][   ][ A ][ B ][ C ][ D ][   ][   ]
   xóa B theo cách ngây thơ:
          [   ][   ][ A ][   ][ C ][ D ][   ][   ]
                          ^ lỗ trống

   get(C): bắt đầu ở ô lý tưởng 3 -> EMPTY -> "KHÔNG THẤY"      SAI! C nằm ở ô 4.
```

Tra cứu dừng ngay ở ô trống đầu tiên, nên lỗ trống (hole) vừa tạo ra sẽ
**cắt đứt chuỗi dò** và khiến `C` lẫn `D` không còn tìm thấy được.

### Cách 1: tombstone (không dùng)

Đánh dấu ô là `DELETED` thay vì `EMPTY`, rồi bỏ qua các tombstone khi tra cứu.
Cách này hoạt động, nhưng tombstone tích tụ dần khiến chuỗi dò dài ra; cuối cùng
cả bảng phải được xây dựng lại, gây ra một đợt tăng vọt độ trễ. Chú thích trong
mã nguồn nêu rõ đây là điều cần tránh.

### Cách 2: xóa bằng cách dịch lùi (dùng ở đây)

Sau khi tạo lỗ trống, **ta tiếp tục dò về phía trước và dịch lùi mọi phần tử
không thì sẽ không còn tìm thấy được**, nhờ đó không cần để lại tombstone.

Quy tắc cho một phần tử đang nằm ở `probe`, có ô lý tưởng là `ideal`:

> Nó có thể chuyển vào lỗ trống nếu đường dò của nó (`ideal → ... → probe`)
> **đi qua lỗ trống**, tức là nếu ô lý tưởng của nó nằm *tại hoặc trước* lỗ
> trống.

Chuyển nó vào đó luôn an toàn: vị trí mới vẫn nằm trên đường dò của chính nó, và
còn gần ô lý tưởng hơn.

"Tại hoặc trước" cần được hiểu cẩn thận vì bảng là một **vòng tròn**: sau ô
cuối cùng là ô 0. Quy tắc chính xác để xác định khi nào một phần tử phải **ở
lại** là:

> Xét đường đi vòng từ sau lỗ trống đến ô `probe` của phần tử, có tính ô
> đó. Nếu ô `ideal` nằm trên đường đi đó, phần tử **ở lại**: phép tra cứu
> bắt đầu *sau* lỗ trống nên sẽ không đi qua lỗ. Ngược lại, phần tử **chuyển**
> vào lỗ trống.

Hai chi tiết nữa để hoàn chỉnh thuật toán:

- Một phần tử ở lại **không** kết thúc lượt quét. Các phần tử phía sau vẫn có
  thể có ô lý tưởng nằm trước lỗ trống, nên ta tiếp tục (`F` trong ví dụ bên
  dưới).
- Lượt quét kết thúc ở ô `EMPTY` đầu tiên: không chuỗi dò nào đi tiếp qua một ô
  trống, nên không có gì phía sau nó phụ thuộc vào lỗ trống. (Mã nguồn cũng dừng
  nếu `probe` đi trọn một vòng quay lại `hole`. Điều đó không thể xảy ra khi bảng
  luôn giữ ít nhất một ô trống, mà 5.5 đã đảm bảo, nhưng nó làm cho vòng lặp hiển
  nhiên là hữu hạn.)

**Ví dụ:** xóa `B` khỏi `[_, _, A, B, C, D, F, _]`, trong đó `F` nằm đúng ô lý
tưởng của nó là ô 6.

```text
 ban đầu ô:     0  1  2  3  4  5  6  7
                _  _  A  B  C  D  F  _          hole := 3 (đã xóa B)
                _  _  A  .  C  D  F  _

 probe 4: C, ideal 3 <= hole 3  -> đường dò của nó đi qua lỗ trống -> CHUYỂN vào 3, hole := 4
                _  _  A  C  .  D  F  _

 probe 5: D, ideal 2 <= hole 4  -> CHUYỂN vào 4, hole := 5
                _  _  A  C  D  .  F  _

 probe 6: F, ideal 6 KHÔNG <= hole 5 -> đường dò của nó không bao giờ chạm lỗ trống -> Ở LẠI
 probe 7: EMPTY -> dừng.

 kết quả        _  _  A  C  D  .  F  _          mọi phần tử vẫn tới được
```

Bảng là một vòng tròn, nên phép thử "tại hoặc trước" phải đúng cả khi một chuỗi
dò quấn từ cuối mảng về đầu mảng. Đó là lý do mã nguồn có hai nhánh:

```rust
let displaced = if probe >= hole {
    ideal <= hole || ideal > probe        // normal case (or the path wrapped around)
} else {
    ideal <= hole && ideal > probe        // `probe` has wrapped past the end
};
```

(Biến có tên `displaced`; hãy hiểu nó là **"có thể được chuyển lùi vào
lỗ trống"**.)

**Ví dụ quấn vòng** (8 ô): `P` (ô lý tưởng 6) ở ô 6, `Q` (ô lý tưởng 7) ở ô 7,
`R` (ô lý tưởng 6) bị quấn sang ô 0. Xóa `P` (hole = 6). Ta vẽ nó thành vòng tròn
đúng như bản chất:

```text
                 ô 7: Q (ideal 7)
                  ^              \
                 /                v
   ô 6: hole (đã xóa P)         ô 0: R (ideal 6)
                 ^                |
                  \              v
          ô 2 đến 5  <--     ô 1: EMPTY

   việc dò đi theo chiều kim đồng hồ: 6 -> 7 -> 0 -> 1 -> ...
```

- Đường đi của `Q` (ở ô 7): từ lỗ trống, không tính lỗ trống, tới ô 7 chỉ có {7}.
  Ô lý tưởng 7 của `Q` nằm trên đường đi đó, nên `Q` **ở lại**.
- Đường đi của `R` (ở ô 0): {7, 0}. Ô lý tưởng 6 của `R` không nằm trên đó (6
  chính là lỗ trống), nên `R` **chuyển** vào ô 6.

Nhìn theo từng bước trong code:

- Probe 7: `Q`, ideal 7; `7 <= 6`? không, `7 > 7`? không → ở lại.
- Probe 0 (đã quấn vòng, `probe < hole`): `R`, ideal 6; `6 <= 6 && 6 > 0` →
  **chuyển** vào ô 6. Lúc này hole là 0.
- Probe 1 là `EMPTY` → dừng.

## 5.7 Độ phức tạp

| Thao tác | Kỳ vọng | Trường hợp xấu nhất |
|---|---|---|
| `get` | O(1), khoảng 1.5 lần dò | O(capacity) nếu mọi khóa đều va chạm |
| `insert` | O(1) | O(capacity) |
| `remove` | O(1) cộng một đợt dịch lùi ngắn | O(capacity) |

Với tải công việc bình thường thì đây là "O(1)", và các doc comment đều cẩn thận
ghi là "O(1) kỳ vọng" (expected O(1)).

### Id được dựng sẵn, và seed bí mật

Bất kỳ ai **chọn được id lệnh** và đoán trước hàm băm đều có thể gây ra trường
hợp xấu nhất. Khi không có seed, finalizer của SplitMix64 hoàn toàn
**khả nghịch** (invertible): phép cộng một hằng số, `z ^ (z >> k)` và phép nhân
với một hằng số lẻ đều có thể được đảo ngược. Vì thế kẻ tấn công có thể tính ra
hàng nghìn id đều băm về ô 0, và khi đó mọi lần tra cứu phải đi qua một chuỗi dò
rất dài: một kiểu tấn công từ chối dịch vụ (denial of service) rẻ tiền nhắm vào
độ trễ.

Cách phòng thủ chỉ tốn một chỉ thị: XOR một **seed ngẫu nhiên, bí mật** vào khóa
trước khi trộn. `FlatHashMap::with_capacity` lấy seed từ `RandomState` của thư
viện chuẩn; đây cũng là nguồn ngẫu nhiên mà `std` dùng làm khóa cho SipHash. Nếu
không biết seed, kẻ tấn công không thể đảo ngược hàm băm để nhắm id vào một ô.

Test `keys_crafted_for_one_seed_spread_out_under_another` minh họa cả hai trường
hợp. Hàm `craft_key` đảo ngược finalizer để tạo 1,000 id cùng nhắm vào ô 0 với
seed `0`, rồi xác nhận điều đó. Với seed khác, các id ấy phân bố như những khóa
bình thường; test yêu cầu bucket đông nhất chứa ít hơn 32 id.

> [!NOTE]
> **Ngẫu nhiên mà không mất tính tất định.** Seed chỉ thay đổi *vị trí* lưu các
> phần tử, không làm thay đổi giá trị mà `get`, `insert` hay `remove` trả về.
> Vì vậy engine vẫn **tất định** (deterministic): cùng một chuỗi yêu cầu luôn
> tạo ra cùng các sự kiện và cùng trạng thái sổ lệnh, bất kể seed là gì.
> `tests/wal_replay.rs` dựa đúng vào điều này: nó phát lại một log vào engine thứ
> hai, engine này bốc một seed khác, và yêu cầu kết quả giống hệt. Những test cần
> một *cách bố trí* tái lập được thì dùng `FlatHashMap::with_capacity_and_seed`.

## 5.8 Khóa dành riêng

`u64::MAX` là **giá trị đánh dấu đặc biệt** (sentinel) cho một ô trống, nên nó
không thể đồng thời là một khóa. Có hai lớp bảo vệ điều này:

1. **`Engine::process_order`** từ chối `order_id == RESERVED_KEY` ngay từ đầu,
   phát ra `OrderRejected` với lý do `ReservedOrderId`. (Nếu không có kiểm tra
   này, một client gửi id `u64::MAX` sẽ chạm tới lớp 2 bên dưới và làm cả tiến
   trình bị dừng đột ngột (abort): một kiểu từ chối dịch vụ. Còn nếu thiếu *cả
   hai* lớp, phần tử sẽ nằm đó mà âm thầm không ai tìm tới được.)
2. **`FlatHashMap::insert`** có một `assert!` cứng (không phải `debug_assert!`),
   nên ngay cả khi ai đó dùng sai trực tiếp thì nó cũng thất bại ồn ào trong bản
   release.

`get` và `remove` với `u64::MAX` thì vô hại: chúng chỉ báo "không tìm thấy", như
mọi lần miss khác (vòng dò chạy cho tới khi gặp một ô trống). Test
`rejects_reserved_order_id_sentinel` bao phủ đường đi qua engine, còn
`insert_rejects_reserved_sentinel_key_in_all_build_profiles` bao phủ bảng.

## 5.9 Tóm tắt

- Một mảng phẳng các ô `(key, value)`, kích thước là lũy thừa của hai, đầy tối đa
  50%, và không bao giờ đầy hẳn (`insert` từ chối lấp ô trống cuối cùng).
- Băm bằng finalizer của SplitMix64 sau khi XOR với một seed bí mật riêng của
  từng bảng, rồi chọn ô bằng `& mask`.
- Va chạm được giải quyết bằng cách bước sang ô kế tiếp (dò tuyến tính).
- Xóa bằng **dịch lùi** thay vì tombstone, nên sau này không có gì phải dọn.
- Định cỡ một lần, không bao giờ đổi kích thước, không bao giờ cấp phát sau
  `with_capacity`.

## 5.10 Tự thực hành

Chạy các unit test của bảng băm:

```sh
cargo test hashmap::
```

Có hai test đáng đọc kỹ. `survives_many_collisions_and_deletions` kiểm tra cơ
chế xóa dịch lùi ở 5.6, còn `keys_crafted_for_one_seed_spread_out_under_another`
mô phỏng cuộc tấn công ở 5.7 và cho thấy seed ngăn chặn được nó:

```sh
cargo test keys_crafted_for_one_seed_spread_out_under_another
```

**Tiếp theo:** [6. Thuật toán khớp lệnh](06-matching-algorithm.md)
