# 10. Kiểm thử và hiệu năng

> *Bản gốc tiếng Anh: [10-testing-and-performance.md](../10-testing-and-performance.md)*

> **Gặp từ lạ?** Mọi thuật ngữ kỹ thuật trong các chương đều được giải thích
> trong [bảng thuật ngữ](glossary.md).

Nguồn: các unit test nằm trong từng tệp `src/*.rs`, các integration test trong
[`tests/`](../../tests), các doctest,
[`benches/matching_engine_bench.rs`](../../benches/matching_engine_bench.rs)
và [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)

```sh
cargo test                      # everything: unit, integration and doc tests
cargo test --no-fail-fast       # keep going after a failing test binary
cargo test --test model_check   # just the differential test
cargo test --test zero_alloc    # just the zero-allocation proof
cargo test -- --list            # list every test by name
cargo bench                     # criterion micro-benchmarks
```

> [!NOTE]
> Mặc định, `cargo test` dừng khi **binary kiểm thử** đầu tiên gặp lỗi, nên lỗi
> ở test thư viện có thể khiến các integration test không được chạy. Hãy dùng
> `--no-fail-fast`, hoặc chỉ đích danh một binary test như ở trên, khi bạn muốn
> xem mọi kết quả.

## 10.1 Mỗi unit test chứng minh điều gì

Mỗi module có khối `#[cfg(test)] mod tests` riêng ở cuối tệp. Đọc chúng là cách
hay để hiểu những hành vi mà module đó cam kết.

### `engine.rs`

| Test | Chứng minh điều gì |
|---|---|
| `resting_order_with_no_match` | Lệnh không vượt spread thì nằm chờ trong sổ; bid tốt nhất và độ sâu đều đúng; chỉ phát ra `OrderAccepted` (kèm phía, tick và số lượng của lệnh). |
| `acceptance_repeats_the_order_terms_even_when_it_fills_at_once` | `OrderAccepted` mô tả lệnh ngay cả khi lệnh không bao giờ nằm chờ trong sổ, nên chỉ riêng luồng sự kiện cũng cho biết lệnh đã đặt là gì. |
| `immediate_or_cancel_trades_what_it_can_and_expires_the_rest` | Lệnh IOC giao dịch, rồi báo phần còn lại của nó bằng `OrderExpired`; không có gì nằm chờ trong sổ. |
| `immediate_or_cancel_that_cannot_trade_expires_without_a_slot` | Khi arena đã đầy, một lệnh IOC không giao dịch được sẽ hết hạn thay vì nhận `BookFull`: phần còn lại của nó không bao giờ cần đến một ô. |
| `market_buy_sweeps_asks_at_every_price_and_never_rests` | Lệnh mua thị trường bỏ qua tick được truyền vào (kể cả tick ngoài phạm vi), giao dịch ở các tick 0, 2000 và 4095, rồi cho phần còn lại hết hạn. |
| `market_order_on_an_empty_side_only_expires` | Lệnh bán thị trường không có gì để giao dịch cùng thì phát ra `Accepted` (với giới hạn 0) và `Expired`. |
| `post_only_that_would_trade_is_rejected_and_the_book_is_unchanged` | Lệnh mua post-only ở đúng ask tốt nhất, và lệnh bán post-only ở đúng bid tốt nhất, đều bị từ chối với `PostOnlyWouldTrade` trước khi được chấp nhận. |
| `post_only_that_does_not_cross_rests_like_a_limit_order` | Lệnh post-only có giá dưới ask tốt nhất được chấp nhận và nằm chờ trong sổ. |
| `the_account_is_reported_on_acceptance_and_kept_while_resting` | Cả `OrderAccepted` lẫn `resting_order` đều mang theo tài khoản của lệnh. |
| `self_trades_happen_unless_prevention_is_asked_for` | Với chế độ mặc định, `Allow`, một lệnh mua giao dịch với lệnh bán đang chờ của chính tài khoản mình như với bất kỳ lệnh nào khác. |
| `cancel_resting_removes_the_own_order_and_keeps_matching` | `CancelResting` hủy lệnh đang chờ của chính tài khoản sau `SelfTradePrevented`; lệnh mua sau đó cho phần còn lại nằm chờ trong sổ. |
| `cancel_incoming_expires_the_remainder_even_of_a_limit_order` | `CancelIncoming` chặn một lệnh mua giới hạn ngay tại lệnh của chính tài khoản đó: phần còn lại của nó hết hạn, còn lệnh của chính tài khoản vẫn ở lại. |
| `cancel_both_removes_the_own_order_and_stops_the_incoming_one` | `CancelBoth` làm cả hai việc, và để lại một sổ trống. |
| `full_fill_taker_and_maker` | Hai lệnh khớp vừa khít nhau phát ra `Trade`, `Filled(maker)`, `Filled(taker)` và để sổ trống. |
| `partial_fill_leaves_maker_resting` | Taker nhỏ hơn thì khớp hết; maker vẫn nằm chờ với số lượng đã giảm. |
| `partial_fill_leaves_taker_resting_remainder` | Taker lớn hơn thì ăn hết maker, rồi phần còn lại của nó nằm chờ trong sổ. |
| `cancel_head_middle_tail_of_price_queue` | Hủy lệnh ở đầu, giữa và cuối hàng đợi FIFO đều chạy đúng; lần hủy thứ hai trả về `false` và phát ra `CancelRejected`. |
| `cancel_reports_only_what_is_left_after_a_partial_fill` | `OrderCancelled` mang theo phía, tick và số lượng còn đang nằm chờ (6 trên 10), chứ không phải số lượng ban đầu. |
| `resting_order_follows_an_order_through_its_life` | `resting_order` cho thấy số lượng còn lại của lệnh sau khi khớp một phần, và trả về `None` khi lệnh đã bị hủy, với một taker chưa từng nằm chờ, và với id dành riêng. |
| `cancelling_a_filled_order_is_rejected_without_touching_the_book` | Lệnh hủy nhắm vào một lệnh đã khớp hết thì phát ra `CancelRejected` và không động đến độ sâu cũng như số lệnh còn sống. |
| `reducing_an_order_keeps_its_time_priority` | Lệnh bị giảm số lượng phát ra `OrderReduced`, làm giảm độ sâu của mức giá, và vẫn được khớp trước lệnh xếp hàng sau nó. |
| `reducing_by_the_whole_remainder_cancels_the_order` | Giảm nhiều hơn số lượng còn lại thì lệnh bị hủy, kèm `OrderCancelled` cho số lượng còn lại lúc đó. |
| `reduce_rejects_zero_and_orders_that_are_not_resting` | Lần giảm 0 đơn vị và một id không tồn tại nhận `CancelRejected` với `ZeroQuantity` và `NotResting`; số 0 được kiểm tra trước. |
| `cancel_account_removes_only_that_accounts_orders_in_book_order` | `cancel_account` hủy các bid của một tài khoản từ tốt nhất trở đi, rồi đến các ask của nó, và không động đến lệnh của tài khoản kia. |
| `cancel_all_empties_the_book_and_always_reports` | `cancel_all` làm trống sổ, và trên một sổ trống vẫn phát ra `MassCancelled` với số đếm bằng 0. |
| `a_restored_engine_keeps_priority_and_counters` | Snapshot liệt kê các bid từ tốt nhất trở đi, rồi đến các ask, lệnh cũ nhất trước ở mỗi mức giá; engine được khôi phục khớp một cú quét theo đúng thứ tự đó, với trade id tiếp nối chỗ mà engine gốc đã dừng. |
| `restore_refuses_snapshots_no_live_book_could_have` | `restore` từ chối dung lượng bằng 0, quá nhiều lệnh, id trùng, số lượng bằng 0, một mức giá sẽ bị tràn và một sổ bị chéo giá (crossed book), mỗi trường hợp với một `SnapshotError` riêng. |
| `crosses_multiple_ask_price_levels_in_price_time_priority` | Lệnh mua quét qua các ask từ mức rẻ nhất đi lên. |
| `trade_names_the_aggressor_side` | `Trade` của một taker bán báo `taker_side: Sell`. |
| `crosses_multiple_bid_price_levels_for_incoming_sell` | Lệnh bán quét qua các bid từ mức cao nhất đi xuống (trường hợp đối xứng). |
| `trade_ids_count_up_across_calls` | `trade_id` đếm 1, 2, 3 qua các lời gọi; lệnh bị từ chối, lệnh nằm chờ và lần hủy lệnh không tiêu tốn id nào. |
| `price_time_priority_within_a_level_is_fifo` | Trong cùng một giá, lệnh cũ hơn được khớp trước. |
| `rejects_duplicate_order_id_and_zero_qty` | Bước kiểm tra đầu vào từ chối id trùng với một lệnh còn sống (`DuplicateOrderId`) và `qty == 0` (`ZeroQuantity`). |
| `rejects_out_of_range_price_tick` | Tick 4096 bị từ chối với `PriceOutOfRange` và sổ vẫn trống. |
| `input_checks_run_in_a_fixed_order` | Lệnh có nhiều lỗi nhận lý do của bước kiểm tra đầu tiên (`ZeroQuantity`). |
| `depth_snapshot_orders_best_to_worst` | Snapshot được sắp xếp từ tốt nhất đến kém nhất. |
| `depth_queries_accept_any_input_without_panicking` | `depth_at` với tick ngoài phạm vi trả về 0, và `depth_snapshot(side, usize::MAX)` trả về mọi mức giá của cả hai phía. |
| `arena_exhaustion_rejects_resting_order` | Arena đầy thì từ chối lệnh cần nằm chờ (`Accepted`, `Rejected(BookFull)`). |
| `rejects_reserved_order_id_sentinel` | `u64::MAX` bị từ chối với `ReservedOrderId`, và việc hủy id đó là an toàn. |
| `rejects_resting_order_that_would_overflow_level_total_qty` | Một lệnh có số lượng `u64::MAX` làm đầy một mức giá; lệnh kế tiếp ở mức đó bị từ chối với `LevelQuantityOverflow` và không để lại ô nào bị treo (dangling). |
| `deep_sweep_reports_every_execution` | Một taker quét qua 40 maker báo cáo đủ mọi giao dịch và mọi lần khớp qua một sink kiểu `Vec`: đây là test hồi quy (regression test) cho giới hạn 64 sự kiện trước đây. |
| `bounded_batch_sink_reports_truncation` | Cũng cú quét đó nhưng ghi vào một `EventBatch` thì giữ lại 64 sự kiện và đếm 18 sự kiện còn lại là bị bỏ (dropped). |
| `panicking_sink_leaves_the_book_consistent` | Một sink panic ở lần khớp của một maker (bắt bằng `catch_unwind`) không để lại giá tốt nhất lỗi thời (stale) nào. |

### Các module khác

| Module | Các test | Chứng minh điều gì |
|---|---|---|
| `pool.rs` | `the_account_fits_in_existing_padding`, `allocate_and_free_roundtrip`, `exhausts_capacity`, `fifo_queue_push_and_remove_head_mid_tail`, `double_free_is_detected`, `removing_from_an_empty_level_is_detected` | `Order` vẫn giữ kích thước 48 byte; các ô đã giải phóng có thể cấp phát lại và `live_count` theo dõi đúng; dung lượng tối đa được thực thi; danh sách liên kết vẫn nhất quán khi xóa ở đầu, giữa và cuối; giải phóng hai lần (double free) và một thao tác xóa bất khả thi đều panic trong mọi build profile. |
| `bitset.rs` | `empty_bitset_has_no_extrema`, `set_and_query_single_tick`, `tracks_lowest_and_highest_across_groups` | `lowest`/`highest` đúng khi vượt qua ranh giới word 64 tick (các tick 0, 63, 64, 2000, 4095) và sau khi xóa bit. |
| `hashmap.rs` | `insert_get_remove`, `update_existing_key`, `survives_many_collisions_and_deletions`, `insert_rejects_reserved_sentinel_key_in_all_build_profiles`, `get_and_remove_are_safe_for_reserved_sentinel_key`, `keys_crafted_for_one_seed_spread_out_under_another`, `seeded_maps_behave_identically`, `insert_refuses_to_fill_the_last_empty_slot`, `absurd_capacity_is_rejected_instead_of_wrapping` | Các thao tác cơ bản và xóa bằng cách dịch lùi (backward-shift deletion) khi có va chạm; khóa dành riêng được xử lý đúng; các id được chế tạo nhắm vào một seed sẽ phân tán đều dưới seed khác (chương 5); seed không bao giờ làm đổi kết quả; bảng đầy và kích thước vô lý đều báo lỗi rõ ràng thay vì bị treo hay tràn vòng (wrap). |
| `events.rs` | `full_batch_counts_dropped_events_until_cleared`, `closures_and_vectors_are_sinks` | Một `EventBatch` đầy sẽ đếm số sự kiện bị bỏ cho đến khi gọi `clear`; closure và `Vec` dùng được làm sink. |
| `ringbuffer.rs` | `push_pop_single_thread`, `capacity_rounds_up_to_a_power_of_two`, `zero_capacity_is_rejected`, `spsc_across_threads_preserves_order`, `consumer_reports_disconnect_only_after_draining`, `producer_sees_a_dropped_consumer` | Thứ tự FIFO, hành vi khi đầy/rỗng và quay vòng (wrap-around); 100,000 giá trị đi qua hai luồng thật, đủ cả và đúng thứ tự (và test fail nhanh nếu producer dừng); việc ngắt kết nối chỉ được báo khi không còn gì để đọc. |
| `wal.rs` | `encode_decode_roundtrip`, `decode_rejects_zeroed_and_corrupted_records`, `append_grow_finalize_and_read_back`, `dropping_a_writer_finalizes_it`, `finalizing_an_empty_log_keeps_only_the_header`, `create_refuses_to_overwrite_an_existing_log`, `read_stops_at_zeroed_preallocated_tail`, `read_stops_before_a_torn_record`, `read_ignores_a_partial_trailing_record`, `open_append_resumes_after_the_last_valid_record`, `open_append_upgrades_older_logs`, `a_record_for_account_zero_keeps_its_version_2_bytes`, `open_append_refuses_a_log_written_for_another_capacity`, `a_second_writer_is_refused_while_the_first_is_live`, `replay_rebuilds_the_book_with_the_recorded_capacity`, `replay_from_finishes_what_a_snapshot_started`, `rejects_files_without_a_valid_header` | Bản ghi thuộc mọi loại, mọi loại lệnh, mọi chế độ chống tự giao dịch và mọi tài khoản đều mã hóa rồi giải mã vẫn nguyên vẹn; một loại lệnh hay chế độ không xác định, và việc lật một bit ở bất kỳ đâu trong 40 byte, đều bị phát hiện; bản ghi cho tài khoản 0 giống từng byte với bản mà phiên bản 2 đã ghi; việc mở rộng tệp, `finalize` và `Drop` đều để lại đúng phần header cộng các bản ghi; `create` không bao giờ đụng vào một tệp đã tồn tại; các reader dừng ở bản ghi toàn số 0, bị ghi dở hoặc dở dang đầu tiên; `open_append` ghi tiếp sau bản ghi hợp lệ cuối cùng, nâng cấp một log cũ hơn mà nó ghi tiếp (ghi lại dung lượng của log đó), và từ chối một log được ghi cho dung lượng khác mà không đụng vào nó; writer thứ hai nhận `WouldBlock` còn writer thứ nhất vẫn chạy tiếp; `replay` dựng engine với dung lượng đã được ghi lại và từ chối log không có thông tin đó; `replay_from` chỉ áp dụng các bản ghi đứng sau một snapshot; header sai cho lỗi `InvalidData`. |
| `snapshot.rs` | `encode_decode_roundtrip`, `damage_anywhere_is_detected`, `writing_replaces_the_checkpoint_in_one_step` | Checkpoint mã hóa rồi giải mã vẫn nguyên vẹn, dù có lệnh hay không; mọi bit bị lật và mọi lần cắt cụt đều bị phát hiện; việc ghi thay thế checkpoint cũ và không để lại tệp `.tmp` nào, kể cả khi một lần ghi bị sập trước đó đã để lại một tệp như vậy. |

Các test sự cố của WAL không làm sập thứ gì cả. Chúng tự tay ghi ra các
**crash image** (một header hợp lệ, theo sau là những byte do ta chọn: phần đuôi
toàn số 0, một bản ghi bị ghi dở, nửa bản ghi) rồi kiểm tra các reader hiểu
chúng ra sao. Cách này tất định và nhanh, mà vẫn bao phủ những trạng thái mà một
lần sập có thể để lại.

### Doctests

`cargo test` cũng biên dịch và chạy đoạn code nằm trong các comment tài liệu:

| Ở đâu | Kiểm tra gì |
|---|---|
| `README.md` (được `src/lib.rs` include) | Phần quick start và các ví dụ API đều biên dịch và chạy được, nên README không thể lệch khỏi API. |
| Tài liệu module của `src/events.rs` | Ba loại event sink, đúng như chương 7 trình bày. |
| Tài liệu module của `src/ringbuffer.rs` | Một luồng producer và một consumer dừng khi bị ngắt kết nối, cộng thêm hai ví dụ `compile_fail`: nhân bản (clone) một `Producer`, và push qua một `&Producer` dùng chung, đều **không** được biên dịch (mục 8.5). |

Doctest `compile_fail` là cách để kiểm thử một đảm bảo *ở mức kiểu* (type-level):
`cargo test` sẽ fail nếu đoạn code bị cấm đó một ngày nào đó biên dịch được.

## 10.2 Kiểm thử đối chiếu (differential testing): `tests/model_check.rs`

Test viết tay chỉ bao phủ những trường hợp mà tác giả nghĩ ra được; bug thì ẩn
trong các tổ hợp. Vì vậy, test mạnh nhất của repo này so sánh engine với một sổ
lệnh thứ hai, **được cố tình giữ đơn giản**, trên những luồng yêu cầu ngẫu nhiên
rất dài, và đòi hai bên phải khớp nhau ở mọi thứ quan sát được.

Sổ lệnh tham chiếu được viết sao cho dễ thấy là đúng, chứ không phải cho nhanh:
một `BTreeMap` chứa các mức giá của mỗi phía, một `VecDeque` chứa
`(order_id, qty)` cho mỗi mức giá, một `HashMap` chứa các lệnh còn sống và một
bộ đếm giao dịch. Nó đủ ngắn để kiểm tra bằng mắt.

Bộ sinh yêu cầu (`OpGenerator` trong [`tests/common/mod.rs`](../../tests/common/mod.rs))
dùng seed cố định, nên mỗi lần chạy đều tái lập được. Nó tạo ra:

- chủ yếu là các lệnh đặt trong một dải hẹp 16 tick, để các lệnh thường xuyên
  vượt spread của nhau; khoảng ba phần tư là lệnh giới hạn, phần còn lại là lệnh
  immediate-or-cancel, lệnh thị trường và lệnh post-only; mỗi lệnh đến từ một
  trong bốn tài khoản, và cứ năm lệnh thì có một lệnh yêu cầu chống tự giao dịch,
  nên các lệnh thường xuyên gặp lệnh của chính tài khoản mình;
- các lần hủy lệnh và giảm số lượng (cộng lại khoảng 30%), nhắm vào id còn sống,
  id đã khớp hết, id chưa từng được cấp, và cả id dành riêng; lần giảm đôi khi
  bằng 0 và thường bằng toàn bộ phần còn lại hoặc hơn;
- thỉnh thoảng có lần hủy hàng loạt (khoảng một trên 200 yêu cầu), chủ yếu cho
  một tài khoản, đôi khi cho cả sổ lệnh;
- thỉnh thoảng có các đầu vào biên (edge input): số lượng bằng 0, tick ngoài
  phạm vi, id dùng lại, những cú quét qua toàn bộ sổ, và số lượng quá lớn ở các
  tick rất xa khiến tổng của một mức giá bị tràn.

Sau **mỗi** yêu cầu, test so sánh chính xác luồng sự kiện (kể cả trade id), bid
tốt nhất và ask tốt nhất, số lệnh còn sống, độ sâu tại mọi tick mà bộ sinh có thể
dùng, và `resting_order` của lệnh mà yêu cầu nhắc tới cùng mọi maker đã giao dịch
với nó. Cứ mỗi 1,000 yêu cầu, nó còn so sánh toàn bộ `depth_snapshot`. Test chạy 4 seed ×
20,000 yêu cầu với arena chật (64 lệnh, nên phần còn lại của lệnh thường bị từ
chối) và thêm 4 seed với arena rộng rãi (8,192 lệnh, nên sổ trở nên sâu).

Hai chi tiết khiến test này đáng tin:

- **Bộ đếm độ phủ (coverage counter).** Test đếm xem những đường đi thú vị được
  chạm tới bao nhiêu lần (đầu vào bị từ chối, phần còn lại bị từ chối, maker bị
  khớp một phần, hủy lệnh thành công, lời gọi tạo ra hơn 64 sự kiện, phần còn lại
  bị hết hạn, lệnh post-only bị từ chối, lần giảm số lượng, lần tự giao dịch bị
  ngăn chặn, lệnh bị gỡ bởi một lần hủy hàng loạt) và fail nếu có bộ đếm nào bằng
  0. Nếu ai đó sửa bộ sinh khiến một đường đi lặng lẽ không
  còn được chạy tới, thay đổi đó không thể lọt qua mà không ai hay biết.
- **Lỗi tái lập được.** Mọi thông báo assertion đều kèm seed, bước và yêu cầu,
  nên có thể phát lại chính xác một lần fail.

Đây là cách tốt nhất để có niềm tin vào một cấu trúc dữ liệu đã được tối ưu: bản
cài đặt nhanh và bản cài đặt hiển nhiên đúng không bao giờ được phép bất đồng với
nhau.

## 10.3 Tính tất định khi phát lại (replay): `tests/wal_replay.rs`

Test này cho 10,000 yêu cầu được sinh ra chạy qua một engine chính, đồng thời ghi
từng yêu cầu vào một WAL *trước khi* áp dụng nó (với phần cấp phát trước nhỏ, nên
tệp phải lớn lên nhiều lần). Sau đó nó finalize log, đọc lại, kiểm tra các số thứ
tự chạy từ 0 đến 9,999, rồi phát lại các bản ghi vào một engine **thứ hai**. Bản
sao (replica) này phải cho ra đúng cùng các sự kiện (nên mỗi giao dịch có cùng
`trade_id`), cùng số lệnh còn sống và cùng độ sâu.

Bản sao tự dựng bảng băm của mình, với **seed ngẫu nhiên** riêng. Vì vậy test này
cũng chứng minh luôn điều đã nói ở chương 5: seed chỉ đổi chỗ các phần tử được
lưu, không bao giờ đổi việc engine làm. Bản sao được dựng bởi `wal::replay`, từ
dung lượng mà log ghi lại, nên test này cũng kiểm tra rằng header có mang theo
thông tin đó.

`tests/snapshot.rs` đưa ra cùng kiểu khẳng định đó cho snapshot (mục 9.8). Nó
chạy 10,000 yêu cầu được sinh ra, khôi phục một engine thứ hai từ snapshot của
engine thứ nhất, rồi cho cả hai chạy 10,000 yêu cầu kế tiếp: mỗi lời gọi phải tạo
ra đúng cùng các sự kiện, và cuối cùng hai sổ lệnh phải giống hệt nhau. Test chạy
một lần với arena chật và một lần với arena rộng rãi.

## 10.4 Chứng minh "không cấp phát" (zero allocation)

Lời khẳng định *"`process_order`, `reduce_order` và `cancel_order` không bao giờ
cấp phát"* rất dễ viết trong một comment, và cũng dễ vô tình phá vỡ về sau (chỉ cần một
`Vec::new()` hay `.to_vec()` vô tội). Vì thế nó được **kiểm thử**, bởi
`tests/zero_alloc.rs`.

### Kỹ thuật: global allocator có đếm

Rust cho phép một chương trình thay thế bộ cấp phát toàn cục (global allocator)
của nó. Bài test cài một lớp bọc đếm mọi lần cấp phát, rồi chuyển tiếp công việc
sang bộ cấp phát của hệ thống:

```rust
struct CountingAllocator;
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);   // count it...
        System.alloc(layout)                          // ...then really allocate
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;
```

`#[global_allocator]` ảnh hưởng đến **toàn bộ binary test**: mọi `Box`, `Vec` và
`String` ở bất kỳ đâu đều đi qua nó.

### Bản thân bài test

```text
  1. Dựng engine thứ nhất với 5,000 ask có sẵn, và engine thứ hai  <- ở đây được phép cấp phát (chuẩn bị)
     với 500 ask một lot ở cùng một mức giá
  2. before = ALLOC_COUNT                                          <- chụp lại số đếm
  3. 5,000 lệnh mua nằm chờ (không khớp)                           <- phần việc được đo,
     5,000 lệnh mua có giá nằm trong dải ask                          qua một EventBatch dùng lại
     5,000 lần giảm số lượng đi một lot
     5,000 lần hủy lệnh
     một lần cancel_all mọi lệnh còn nằm chờ
     một taker quét hết 500 maker của engine thứ hai,                 qua một closure chỉ đếm
  4. after = ALLOC_COUNT
  5. assert_eq!(after, before) và thấy đủ 500 giao dịch            <- không có lần cấp phát nào ở giữa
```

Những chi tiết đáng bắt chước khi bạn tự viết test kiểu này:

- **Khởi động (warm up) và cấp phát sẵn trước**, để phần chuẩn bị chỉ chạy một
  lần không bị đếm vào.
- **Chạy qua mọi đường đi** mà bạn muốn chứng minh là không cấp phát: nằm chờ,
  khớp lệnh, giảm số lượng, hủy lệnh, hủy hàng loạt, và một cú quét sâu hơn xa
  mọi bộ đệm cố định đều chạy những đoạn code khác nhau.
- **Chọn các sink không cấp phát.** Vì mục tiêu là đo riêng engine, bài test tái
  sử dụng một `EventBatch` (gọi `clear` trước mỗi lần) cùng một closure chỉ đếm.
- **`std::hint::black_box`** bọc quanh kết quả để trình tối ưu không xóa mất
  những lời gọi có kết quả không được dùng.
- **`realloc` cũng được phủ:** `GlobalAlloc::realloc` mặc định gọi `alloc` của
  ta, nên việc một `Vec` phình ra cũng sẽ bị đếm.

Những gì nó **không** chứng minh:

- Bộ đếm ghi nhận mọi lần cấp phát **trong toàn bộ tiến trình**, từ tất cả các
  luồng. Vì vậy, phép đo chỉ chính xác khi đây là test duy nhất trong binary đó
  (tệp có ghi chú điều này), vì test harness chạy các test song song trên nhiều
  luồng.
- Nó chỉ chạy các đường đi nêu trên, chứ không phải mọi đầu vào có thể (ví dụ
  các đường từ chối lệnh).
- `depth_snapshot` *có* cấp phát và cố tình bị loại ra.

Nếu muốn, bạn có thể chạy ở cả hai profile; ở profile nào nó cũng pass:

```sh
cargo test --test zero_alloc
cargo test --release --test zero_alloc
```

## 10.5 Các build profile

Trích từ `Cargo.toml`:

```toml
[profile.release]
opt-level = 3        # full optimisation
lto = true           # optimise across crate boundaries (including dependencies like memmap2)
codegen-units = 1    # one big compilation unit: slower build, better optimisation
panic = "abort"      # no unwinding tables; a panic terminates the process (see below)

[profile.bench]
opt-level = 3
lto = true
```

- **`panic = "abort"`** thể hiện triết lý của dự án này: khi một bất biến bị vi
  phạm (một `expect` trong `PriceLevel`, các `assert!` trong `FlatHashMap::insert`
  và `OrderPool::free`), tiến trình nên dừng ngay thay vì unwind (tháo ngăn
  xếp), đúng với nguyên tắc "thà dừng lại và báo lỗi rõ ràng còn hơn chạy tiếp
  với một sổ lệnh đã hỏng". Khi đó, việc khôi phục là phát lại WAL (chương 9).

  > [!NOTE]
  > **Thiết lập này chỉ có hiệu lực trong một binary bạn build từ chính
  > package này.** `matching_engine` là một thư viện. Cargo bỏ qua các thiết lập
  > `[profile]` của dependency, nên một chương trình *dùng* crate này phải đặt
  > `panic = "abort"` trong `Cargo.toml` **của chính nó** thì mới có hành vi này.
  > Cargo cũng bỏ qua thiết lập `panic` cho test và benchmark, vốn luôn unwind.
  > (Đó là lý do các test như `double_free_is_detected` dùng được
  > `#[should_panic]`, và `panicking_sink_leaves_the_book_consistent` bắt được
  > một panic.)
- **`lto` và `codegen-units = 1`** cho phép trình biên dịch tối ưu xuyên qua
  ranh giới giữa các crate. Điều này quan trọng với các lời gọi từ benchmark hay
  từ binary của bạn vào thư viện (và từ thư viện vào `memmap2`).
  `#[inline(always)]` vốn đã hoạt động bên trong một crate mà không cần chúng.
- Để trình biên dịch dùng các lệnh quét bit mới nhất (`tzcnt`, `lzcnt`; xem
  [chương 4](04-price-ladder-bitset.md)), hãy build cho đúng CPU của bạn:

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench          # bash / zsh
```

```powershell
$env:RUSTFLAGS = "-C target-cpu=native"; cargo bench  # PowerShell
```

(Một binary build theo cách này có thể không chạy được trên các CPU cũ hơn.)

## 10.6 Các benchmark

[`benches/matching_engine_bench.rs`](../../benches/matching_engine_bench.rs) dùng
thư viện [criterion](https://docs.rs/criterion). Có ba benchmark, mỗi cái chạy
trên một engine mới:

| Benchmark | Chuẩn bị (không tính giờ) | Phần việc được tính giờ | Số lệnh mỗi lần chạy |
|---|---|---|---|
| `place_10k_resting_orders` | engine rỗng, dung lượng 20,000 | 10,000 lệnh mua, mỗi lệnh ở tick `i % 4000`; không lệnh nào vượt spread nên lệnh nào cũng nằm chờ | 10,000 |
| `cross_spread_matching` | 5,000 ask cỡ 1 ở tick 100 | 5,000 lệnh mua cỡ 1 ở tick 100; mỗi lệnh ăn đúng một maker | 5,000 |
| `cancel_orders` | 10,000 lệnh mua đang chờ | hủy cả 10,000 lệnh theo id | 10,000 |

### Criterion đo như thế nào

```rust
b.iter_batched(
    || (Engine::new(20_000), EventBatch::new()),   // setup: fresh state, NOT timed
    |(mut engine, mut events)| {                    // routine: timed
        for i in 0..10_000u64 {
            events.clear();                          // one reused sink, as a real caller would
            engine.process_order(i, Side::Buy, (i % 4000) as u32, 10, &mut events);
            black_box(&events);
        }
        engine                                       // returned, so it is dropped outside the timing
    },
    BatchSize::LargeInput,                           // "setup output is big, don't batch many together"
);
```

- **`iter_batched`** dựng trạng thái mới cho mỗi lần chạy routine, nên thời gian
  đo không gồm `Engine::new` và mỗi lần chạy đều bắt đầu từ cùng một trạng thái.
  Việc trả `engine` ra khỏi routine khiến việc hủy nó diễn ra ngoài vùng tính
  giờ.
- **`black_box`** ngăn trình biên dịch tối ưu bỏ mất phần việc.
- Đầu tiên criterion **khởi động** (3 giây), rồi thu 100 mẫu và báo cáo một
  **khoảng tin cậy** (confidence interval): `time: [low estimate high]`. Giá trị
  "estimate" là ước lượng độ dốc hồi quy (regression slope) của criterion cho
  **thời gian của một lần chạy routine**, nghĩa là cả lô 5,000 hoặc 10,000 lệnh,
  chứ không phải một lệnh.

Muốn có con số cho mỗi lệnh, hãy **chia cho số lệnh**.

### Một lần chạy mẫu (chỉ để tham khảo)

Đo trên chiếc máy dùng để viết các tài liệu này: Intel Core i9-12900H, Windows
11, Rust 1.98.1, `cargo bench`, chạy một lần, không cố làm cho máy yên tĩnh.

| Benchmark | Thời gian báo cáo (estimate) | Số lệnh | ≈ mỗi lệnh |
|---|---|---|---|
| `place_10k_resting_orders` | 495 µs | 10,000 | ≈ 50 ns |
| `cross_spread_matching` | 346 µs | 5,000 | ≈ 69 ns |
| `cancel_orders` | 403 µs | 10,000 | ≈ 40 ns |

Phiên bản trước của chương này ghi nhận 746, 390 và 734 µs trên cùng chiếc máy
đó, với API cũ trả về một `EventBatch` 2 KiB mới tinh từ mỗi lời gọi. Criterion
so sánh mỗi lần chạy với lần trước và báo các con số mới là cải thiện khoảng 36%,
13% và 46%. Các thao tác rẻ nhất được lợi nhiều nhất, đúng như điều ta mong đợi
nếu việc khởi tạo và sao chép batch đó chiếm một phần lớn chi phí của chúng: đây
là một bằng chứng đo được cho điểm đã nói ở chương 7, rằng "không cấp phát" không
có nghĩa là "không tốn chi phí". Vì chỉ có một lần chạy trên laptop, hãy hiểu đây
là "nhanh hơn rõ rệt", đừng coi các tỷ lệ phần trăm là chính xác.

> [!IMPORTANT]
> Hãy coi các số này là bản phác thảo về bậc độ lớn, **không phải một lời hứa**.
> Phần cứng, hệ điều hành, thiết lập nguồn điện và tải nền của bạn đều làm các
> con số thay đổi, và không nên đem chúng so sánh giữa các máy.

### Những con số này nói được và không nói được gì

- Đây là **giá trị trung bình trên cả lô**, tức *thông lượng*. Chúng không nói gì
  về **đuôi**: 1 lệnh chậm nhất trong 1,000 lệnh. Với một hệ thống nhạy cảm về
  độ trễ, đuôi mới là thứ quan trọng nhất; muốn đo nó cần một bộ đếm giờ cho từng
  lời gọi, ghi kết quả vào một histogram (ví dụ crate `hdrhistogram`).
- Khối lượng công việc là **tổng hợp** (synthetic): các lệnh xoay vòng qua vài
  tick, tập dữ liệu đang dùng (working set) nhỏ và nóng trong cache, và không có
  mạng, không có WAL. Lưu lượng thật lộn xộn hơn nhiều.
- Mỗi lời gọi ghi sự kiện của nó vào một `EventBatch` dùng lại. Phiên bản trước
  của engine thì trả về theo giá trị một batch 2 KiB mới từ mỗi lời gọi; API sink
  đã loại bỏ lần sao chép đó khỏi mọi lệnh (chương 7).
- Criterion được cấu hình với `default-features = false` (tính năng
  `html_reports` của nó bị tắt), nên nó chỉ in văn bản; dữ liệu thô vẫn được ghi
  dưới `target/criterion/<benchmark>/`.
- Criterion có thể cảnh báo *"Unable to complete 100 samples in 5.0s"*. Cảnh báo
  này vô hại: mỗi routine đều dài (hàng nghìn lệnh), nên nó chỉ đơn giản là mất
  lâu hơn một chút.

## 10.7 Tích hợp liên tục (CI)

Mỗi lần push và pull request đều chạy
[`.github/workflows/ci.yml`](../../.github/workflows/ci.yml):

| Job | Chạy trên | Kiểm tra gì |
|---|---|---|
| **Test** | `ubuntu-latest`, `windows-latest`, `ubuntu-24.04-arm`, Rust 1.98.1 | `cargo fmt --check`; `clippy -D warnings`; `cargo doc` với `-D warnings`; `cargo test` ở chế độ debug và release; các benchmark biên dịch được; các ví dụ `pipeline`, `walkthrough` và `checkpoint` chạy được |
| **MSRV** (phiên bản Rust tối thiểu được hỗ trợ) | `ubuntu-latest`, Rust 1.89 | `cargo check --lib`: lời hứa `rust-version` trong `Cargo.toml` |
| **Miri** | `ubuntu-latest`, nightly | `cargo miri test --lib`: tìm hành vi không xác định (undefined behavior) trong code `unsafe` (các test có ánh xạ tệp thì bị bỏ qua) |

Mỗi lựa chọn tồn tại để bắt một loại lỗi cụ thể:

- **Windows** ánh xạ tệp vào bộ nhớ theo cách khác (nó từ chối thu nhỏ một tệp
  đang được ánh xạ), nên WAL được kiểm thử ở đó cũng như trên Linux.
- **ARM** sắp xếp lại các truy cập bộ nhớ thoải mái hơn x86 rất nhiều. Một bộ
  đệm vòng thiếu `Acquire` thường vẫn pass trên x86 mà có thể fail trên ARM, nên
  test SPSC đa luồng cũng chạy trên phần cứng ARM thật. Cách đó vẫn chỉ mang tính
  xác suất; `loom` (chương 11) mới là kiểm tra vét cạn.
- **Một toolchain được ghim phiên bản, một `Cargo.lock` được commit, và
  `--locked`** giúp CI tái lập được: một lint mới của trình biên dịch hay một bản
  phát hành dependency mới không thể khiến build chuyển đỏ nếu không có thay đổi
  nào trong repo này. Việc nâng cấp trở thành những commit có chủ đích.
- **Timeout** khiến một test bị treo sẽ fail sau vài phút, thay vì vài giờ.
- **Doctest của README và `tests/docs_sync.rs`** giữ cho tài liệu luôn trung
  thực: các ví dụ trong README và chương trình hoàn chỉnh ở chương 9 đều được
  biên dịch và chạy, nên chúng không thể lặng lẽ lỗi thời.

## 10.8 Những gì test *không* bao phủ

Biết những lỗ hổng là một phần của việc đọc test một cách có phê phán.

| Lỗ hổng | Vì sao quan trọng | Cách lấp |
|---|---|---|
| **Bug về thứ tự bộ nhớ chỉ bị bắt theo xác suất** | Job ARM chạy test bộ đệm vòng đa luồng trên một CPU có thứ tự bộ nhớ yếu, và Miri kiểm tra code `unsafe`, nhưng cả hai chỉ thấy những cách đan xen giữa các luồng tình cờ xảy ra. | Trình kiểm tra mô hình [`loom`](https://docs.rs/loom) vét cạn mọi cách đan xen (một bài tập ở chương 11). |
| **Sự cố thật và mất điện** | Các test WAL dùng crash image dựng tay; không có gì giết tiến trình giữa lúc đang ghi hay cắt điện. | Một harness chạy writer trong tiến trình con rồi giết nó ở những thời điểm ngẫu nhiên; kiểm thử mất điện cần một máy ảo hoặc một thiết bị khối có chèn lỗi (fault-injecting block device). |
| **Suy giảm hiệu năng (performance regression)** | CI chỉ *biên dịch* các benchmark, nên một sự chậm đi sẽ không ai hay biết. | Baseline của Criterion (`cargo bench -- --save-baseline before`, về sau dùng `--baseline before`), hoặc một job benchmark chạy trên một máy chuyên dụng, ít tải. |
| **Các trường hợp biên của `depth_snapshot`** | `model_check` so sánh toàn bộ snapshot và một unit test phủ `usize::MAX`, nhưng `levels = 0` và "ít mức giá hơn số mức hiện có" thì chưa được test. | Hai unit test nhỏ. |
| **Hai writer trên cùng một tệp WAL** | Tài liệu ghi là không được hỗ trợ, nhưng không có gì thực thi hay kiểm thử điều đó. | Một khóa tệp (`File::try_lock`) và một test cho nó (chương 11). |

## 10.9 Làm tiếp việc tối ưu hiệu năng từ đây

| Câu hỏi | Công cụ |
|---|---|
| Thời gian đi đâu mất? | Một profiler lấy mẫu (sampling profiler): `perf` + flamegraph trên Linux, profiler của Visual Studio hoặc `samply` trên Windows. |
| Trình biên dịch có thật sự sinh ra `tzcnt` không? | `cargo asm` / `cargo show-asm`, hoặc Godbolt. |
| Các đường đi khác có cấp phát ngầm không? | Counting allocator (bộ cấp phát có đếm) ở mục 10.4; `heaptrack` / DHAT. |
| Hành vi của cache có phải nút thắt cổ chai không? | Bộ đếm cache-miss của `perf stat`, `valgrind --tool=cachegrind`. |
| Đuôi của độ trễ trông như thế nào? | Ghi thời lượng của từng lời gọi vào một HdrHistogram. |
| Bộ đệm vòng có thật sự đúng trên các mô hình bộ nhớ yếu không? | Job CI chạy trên ARM và Miri (cả hai đã có sẵn); `loom` để kiểm tra vét cạn. |

**Tiếp theo:** [11. Giới hạn và bài tập](11-limitations-and-exercises.md)
