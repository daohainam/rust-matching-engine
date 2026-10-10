# 10. Kiểm thử và hiệu năng

> *Bản gốc tiếng Anh: [10-testing-and-performance.md](../10-testing-and-performance.md)*

> **Gặp từ lạ?** Mọi thuật ngữ kỹ thuật trong các chương đều được giải thích
> trong [bảng thuật ngữ](glossary.md).

Mã nguồn: các kiểm thử đơn vị nằm trong từng tệp `src/*.rs`, các kiểm thử tích hợp trong
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
> Mặc định, `cargo test` dừng khi **tệp thực thi kiểm thử** đầu tiên gặp lỗi, nên lỗi
> ở kiểm thử thư viện có thể khiến các kiểm thử tích hợp không được chạy. Hãy dùng
> `--no-fail-fast`, hoặc chỉ đích danh một tệp thực thi kiểm thử như ở trên, khi bạn muốn
> xem mọi kết quả.

## 10.1 Mỗi kiểm thử đơn vị kiểm tra điều gì

Mỗi mô-đun có khối `#[cfg(test)] mod tests` riêng ở cuối tệp. Đọc chúng là cách
hay để hiểu những hành vi mà mô-đun đó cam kết.

### `engine.rs`

| Kiểm thử | Hành vi được kiểm tra |
|---|---|
| `resting_order_with_no_match` | Lệnh không vượt spread thì nằm chờ trong sổ; bid tốt nhất và độ sâu đều đúng; chỉ phát ra `OrderAccepted` (kèm phía, tick và số lượng của lệnh). |
| `acceptance_repeats_the_order_terms_even_when_it_fills_at_once` | `OrderAccepted` mô tả lệnh ngay cả khi lệnh không bao giờ nằm chờ trong sổ, nên chỉ riêng luồng sự kiện cũng cho biết lệnh đã đặt là gì. |
| `immediate_or_cancel_trades_what_it_can_and_expires_the_rest` | Lệnh IOC giao dịch, rồi báo phần còn lại của nó bằng `OrderExpired`; không có gì nằm chờ trong sổ. |
| `immediate_or_cancel_that_cannot_trade_expires_without_a_slot` | Khi arena đã đầy, một lệnh IOC không giao dịch được sẽ hết hạn thay vì nhận `BookFull`: phần còn lại của nó không bao giờ cần đến một ô. |
| `market_buy_sweeps_asks_at_every_price_and_never_rests` | Lệnh mua thị trường bỏ qua tick được truyền vào (kể cả tick ngoài phạm vi), giao dịch ở các tick 0, 2000 và 4095, rồi cho phần còn lại hết hạn. |
| `market_order_on_an_empty_side_only_expires` | Lệnh bán thị trường không có lệnh đối diện để khớp thì phát ra `Accepted` (với giới hạn 0) và `Expired`. |
| `post_only_that_would_trade_is_rejected_and_the_book_is_unchanged` | Lệnh mua post-only ở đúng ask tốt nhất, và lệnh bán post-only ở đúng bid tốt nhất, đều bị từ chối với `PostOnlyWouldTrade` trước khi được chấp nhận. |
| `post_only_that_does_not_cross_rests_like_a_limit_order` | Lệnh post-only có giá dưới ask tốt nhất được chấp nhận và nằm chờ trong sổ. |
| `the_account_is_reported_on_acceptance_and_kept_while_resting` | Cả `OrderAccepted` lẫn `resting_order` đều mang theo tài khoản của lệnh. |
| `self_trades_happen_unless_prevention_is_asked_for` | Với chế độ mặc định, `Allow`, một lệnh mua giao dịch với lệnh bán đang chờ của chính tài khoản mình như với bất kỳ lệnh nào khác. |
| `cancel_resting_removes_the_own_order_and_keeps_matching` | `CancelResting` hủy lệnh đang chờ của chính tài khoản sau `SelfTradePrevented`; lệnh mua sau đó cho phần còn lại nằm chờ trong sổ. |
| `cancel_incoming_expires_the_remainder_even_of_a_limit_order` | `CancelIncoming` chặn một lệnh mua giới hạn ngay tại lệnh của chính tài khoản đó: phần còn lại của nó hết hạn, còn lệnh của chính tài khoản vẫn ở lại. |
| `cancel_both_removes_the_own_order_and_stops_the_incoming_one` | `CancelBoth` làm cả hai việc, và để lại một sổ trống. |
| `full_fill_taker_and_maker` | Hai lệnh khớp vừa đủ số lượng phát ra `Trade`, `Filled(maker)`, `Filled(taker)` và để sổ trống. |
| `partial_fill_leaves_maker_resting` | Taker nhỏ hơn thì khớp hết; maker vẫn nằm chờ với số lượng đã giảm. |
| `partial_fill_leaves_taker_resting_remainder` | Taker lớn hơn thì khớp hết số lượng của maker, rồi phần còn lại của nó nằm chờ trong sổ. |
| `cancel_head_middle_tail_of_price_queue` | Hủy lệnh ở đầu, giữa và cuối hàng đợi FIFO đều chạy đúng; lần hủy thứ hai trả về `false` và phát ra `CancelRejected`. |
| `cancel_reports_only_what_is_left_after_a_partial_fill` | `OrderCancelled` mang theo phía, tick và số lượng còn đang nằm chờ (6 trên 10), chứ không phải số lượng ban đầu. |
| `resting_order_follows_an_order_through_its_life` | `resting_order` cho thấy số lượng còn lại của lệnh sau khi khớp một phần, và trả về `None` khi lệnh đã bị hủy, với một taker chưa từng nằm chờ, và với ID dành riêng. |
| `cancelling_a_filled_order_is_rejected_without_touching_the_book` | Lệnh hủy nhắm vào một lệnh đã khớp hết thì phát ra `CancelRejected` và không làm thay đổi độ sâu cũng như số lệnh đang hoạt động. |
| `reducing_an_order_keeps_its_time_priority` | Lệnh bị giảm số lượng phát ra `OrderReduced`, làm giảm độ sâu của mức giá, và vẫn được khớp trước lệnh xếp hàng sau nó. |
| `reducing_by_the_whole_remainder_cancels_the_order` | Giảm nhiều hơn số lượng còn lại thì lệnh bị hủy, kèm `OrderCancelled` cho số lượng còn lại lúc đó. |
| `reduce_rejects_zero_and_orders_that_are_not_resting` | Lần giảm 0 đơn vị và một ID không tồn tại nhận `CancelRejected` với `ZeroQuantity` và `NotResting`; số 0 được kiểm tra trước. |
| `cancel_account_removes_only_that_accounts_orders_in_book_order` | `cancel_account` hủy các bid của một tài khoản từ tốt nhất trở đi, rồi đến các ask của nó, và không làm thay đổi lệnh của tài khoản kia. |
| `cancel_all_empties_the_book_and_always_reports` | `cancel_all` làm trống sổ, và trên một sổ trống vẫn phát ra `MassCancelled` với số lượng bằng 0. |
| `a_restored_engine_keeps_priority_and_counters` | Snapshot liệt kê các bid từ tốt nhất trở đi, rồi đến các ask, lệnh cũ nhất trước ở mỗi mức giá; bộ khớp lệnh được khôi phục xử lý lệnh quét qua nhiều mức giá theo đúng thứ tự đó, với ID giao dịch tiếp nối bộ đếm của bộ khớp lệnh gốc. |
| `restore_refuses_snapshots_no_live_book_could_have` | `restore` từ chối dung lượng bằng 0, quá nhiều lệnh, ID trùng, số lượng bằng 0, một mức giá sẽ bị tràn và một sổ bị chéo giá (crossed book), mỗi trường hợp với một `SnapshotError` riêng. |
| `a_wide_ladder_trades_and_restores_far_beyond_4096_ticks` | Một bộ khớp lệnh có 300,000 tick cho lệnh nằm chờ và tìm thấy chúng ở các tick 100, 4096 và 250,000, từ chối tick 300,000, giữ nguyên số tick của nó qua một snapshot, và cho một lệnh mua thị trường mức giới hạn 299,999. |
| `risk_limits_reject_oversized_orders_and_prices_far_through_the_market` | `max_order_qty` và biên độ giá từ chối lệnh với lý do tương ứng, ở cả hai phía; một lệnh nằm đúng ở mép của biên độ vẫn giao dịch, và các lệnh đang chờ không bị ảnh hưởng. |
| `the_price_band_stops_a_market_order_sweep` | Với biên độ 5, một lệnh mua thị trường gặp các ask ở 100, 101 và 150 nhận mức giới hạn 105, giao dịch ở 100 và 101, rồi cho phần còn lại hết hạn; một snapshot giữ lại các giới hạn. |
| `crosses_multiple_ask_price_levels_in_price_time_priority` | Lệnh mua quét qua các ask từ mức thấp nhất đi lên. |
| `trade_names_the_aggressor_side` | `Trade` của một taker bán báo `taker_side: Sell`. |
| `crosses_multiple_bid_price_levels_for_incoming_sell` | Lệnh bán quét qua các bid từ mức cao nhất đi xuống (trường hợp đối xứng). |
| `trade_ids_count_up_across_calls` | `trade_id` đếm 1, 2, 3 qua các lời gọi; lệnh bị từ chối, lệnh nằm chờ và lần hủy lệnh không tiêu tốn ID nào. |
| `price_time_priority_within_a_level_is_fifo` | Trong cùng một giá, lệnh cũ hơn được khớp trước. |
| `rejects_duplicate_order_id_and_zero_qty` | Bước kiểm tra đầu vào từ chối ID trùng với một lệnh đang hoạt động (`DuplicateOrderId`) và `qty == 0` (`ZeroQuantity`). |
| `rejects_out_of_range_price_tick` | Tick 4096 bị từ chối với `PriceOutOfRange` và sổ vẫn trống. |
| `input_checks_run_in_a_fixed_order` | Lệnh có nhiều lỗi nhận lý do của bước kiểm tra đầu tiên (`ZeroQuantity`). |
| `depth_snapshot_orders_best_to_worst` | Snapshot được sắp xếp từ tốt nhất đến kém nhất. |
| `depth_queries_accept_any_input_without_panicking` | `depth_at` với tick ngoài phạm vi trả về 0, và `depth_snapshot(side, usize::MAX)` trả về mọi mức giá của cả hai phía. |
| `arena_exhaustion_rejects_resting_order` | Arena đầy thì từ chối lệnh cần nằm chờ (`Accepted`, `Rejected(BookFull)`). |
| `rejects_reserved_order_id_sentinel` | `u64::MAX` bị từ chối với `ReservedOrderId`, và việc hủy ID đó là an toàn. |
| `rejects_resting_order_that_would_overflow_level_total_qty` | Một lệnh có số lượng `u64::MAX` làm đầy một mức giá; lệnh kế tiếp ở mức đó bị từ chối với `LevelQuantityOverflow` và không để lại ô nào bị treo (dangling). |
| `deep_sweep_reports_every_execution` | Một taker quét qua 40 maker báo cáo đủ mọi giao dịch và mọi lần khớp qua một sink kiểu `Vec`: đây là kiểm thử hồi quy (regression test) cho giới hạn 64 sự kiện trước đây. |
| `bounded_batch_sink_reports_truncation` | Cùng lệnh quét đó nhưng ghi vào một `EventBatch` thì giữ lại 64 sự kiện và đếm 18 sự kiện còn lại là bị bỏ (dropped). |
| `panicking_sink_leaves_the_book_consistent` | Một sink panic ở lần khớp của một maker (bắt bằng `catch_unwind`) không để lại giá tốt nhất lỗi thời (stale) nào. |

### Các mô-đun khác

| Mô-đun | Các kiểm thử | Hành vi được kiểm tra |
|---|---|---|
| `pool.rs` | `the_account_fits_in_existing_padding`, `allocate_and_free_roundtrip`, `exhausts_capacity`, `fifo_queue_push_and_remove_head_mid_tail`, `double_free_is_detected`, `removing_from_an_empty_level_is_detected` | `Order` vẫn giữ kích thước 48 byte; các ô đã giải phóng có thể cấp phát lại và `live_count` theo dõi đúng; dung lượng tối đa được thực thi; danh sách liên kết vẫn nhất quán khi xóa ở đầu, giữa và cuối; giải phóng hai lần (double free) và một thao tác xóa bất khả thi đều panic trong mọi cấu hình biên dịch. |
| `bitset.rs` | `empty_bitset_has_no_extrema`, `set_and_query_single_tick`, `tracks_lowest_and_highest_across_groups`, `the_number_of_levels_follows_the_number_of_ticks`, `a_ladder_too_long_for_four_levels_is_rejected`, `agrees_with_an_ordered_set_at_every_size` | `lowest`/`highest` đúng khi vượt qua ranh giới word 64 tick (các tick 0, 63, 64, 2000, 4095) và sau khi xóa bit; bitset dùng từ 1 đến 4 tầng, tùy số tick cần hỗ trợ; và ở tám kích thước từ 1 đến 300,000 tick, hàng nghìn lần đặt và xóa bit ngẫu nhiên cho cùng kết quả với một `BTreeSet`. |
| `hashmap.rs` | `insert_get_remove`, `update_existing_key`, `survives_many_collisions_and_deletions`, `insert_rejects_reserved_sentinel_key_in_all_build_profiles`, `get_and_remove_are_safe_for_reserved_sentinel_key`, `keys_crafted_for_one_seed_spread_out_under_another`, `seeded_maps_behave_identically`, `insert_refuses_to_fill_the_last_empty_slot`, `absurd_capacity_is_rejected_instead_of_wrapping` | Các thao tác cơ bản và xóa bằng cách dịch lùi (backward-shift deletion) khi có va chạm; khóa dành riêng được xử lý đúng; các ID được cố ý tạo để gây va chạm với một seed sẽ phân tán đều khi dùng seed khác (chương 5); seed không bao giờ làm đổi kết quả; bảng đầy và kích thước vô lý đều báo lỗi rõ ràng thay vì bị treo hay tràn vòng (wrap). |
| `events.rs` | `full_batch_counts_dropped_events_until_cleared`, `closures_and_vectors_are_sinks` | Một `EventBatch` đầy sẽ đếm số sự kiện bị bỏ cho đến khi gọi `clear`; closure và `Vec` dùng được làm sink. |
| `ringbuffer.rs` | `push_pop_single_thread`, `capacity_rounds_up_to_a_power_of_two`, `zero_capacity_is_rejected`, `spsc_across_threads_preserves_order`, `consumer_reports_disconnect_only_after_draining`, `producer_sees_a_dropped_consumer` | Thứ tự FIFO, hành vi khi đầy/rỗng và quay vòng (wrap-around); 100,000 giá trị đi qua hai luồng thực tế, đầy đủ và đúng thứ tự (và kiểm thử báo lỗi ngay nếu producer dừng); việc ngắt kết nối chỉ được báo khi không còn gì để đọc. |
| `wal.rs` | `encode_decode_roundtrip`, `decode_rejects_zeroed_and_corrupted_records`, `append_grow_finalize_and_read_back`, `dropping_a_writer_finalizes_it`, `finalizing_an_empty_log_keeps_only_the_header`, `create_refuses_to_overwrite_an_existing_log`, `read_stops_at_zeroed_preallocated_tail`, `read_stops_before_a_torn_record`, `read_ignores_a_partial_trailing_record`, `open_append_resumes_after_the_last_valid_record`, `open_append_upgrades_older_logs`, `a_record_for_account_zero_keeps_its_version_2_bytes`, `open_append_refuses_a_log_written_for_another_capacity`, `a_second_writer_is_refused_while_the_first_is_live`, `replay_rebuilds_the_book_with_the_recorded_capacity`, `replay_from_finishes_what_a_snapshot_started`, `rejects_files_without_a_valid_header` | Bản ghi thuộc mọi loại, mọi loại lệnh, mọi chế độ chống tự giao dịch và mọi tài khoản đều mã hóa rồi giải mã vẫn nguyên vẹn; một loại lệnh hay chế độ không xác định, và việc lật một bit ở bất kỳ đâu trong 40 byte, đều bị phát hiện; bản ghi cho tài khoản 0 giống từng byte với bản mà phiên bản 2 đã ghi; việc mở rộng tệp, `finalize` và `Drop` đều để lại đúng phần đầu tệp cộng các bản ghi; `create` không bao giờ sửa một tệp đã tồn tại; các bộ đọc dừng ở bản ghi toàn số 0, bị ghi dở hoặc dở dang đầu tiên; `open_append` ghi tiếp sau bản ghi hợp lệ cuối cùng, nâng cấp một nhật ký cũ hơn mà nó ghi tiếp (ghi lại dung lượng của nhật ký đó), và từ chối một nhật ký được ghi cho dung lượng hoặc số tick khác mà không sửa tệp; bộ ghi thứ hai nhận `WouldBlock` còn bộ ghi thứ nhất vẫn chạy tiếp; `replay` dựng bộ khớp lệnh với dung lượng và số tick đã được ghi lại, và từ chối nhật ký không có dung lượng; bản ghi giới hạn rủi ro mã hóa rồi giải mã vẫn nguyên vẹn; `replay_from` chỉ áp dụng các bản ghi đứng sau một snapshot; phần đầu tệp sai cho lỗi `InvalidData`. |
| Phần đầu tệp của `wal.rs` | `header_rejects_every_flipped_bit_and_truncation`, `header_rejects_impossible_engine_dimensions`, `corrupted_header_is_refused_before_replay_or_append` | Mọi bit bị lật trong phần đầu tệp và mọi lần cắt cụt đều bị phát hiện, kể cả thay đổi số phiên bản có thể bị nhầm với định dạng cũ; các kích thước bất khả thi bị từ chối; dung lượng hoặc số tick bị hỏng làm phục hồi dừng trước khi phát sự kiện hay sửa sổ lệnh, và làm thao tác ghi thêm dừng trước khi sửa tệp. |
| `snapshot.rs` | `encode_decode_roundtrip`, `a_version_1_checkpoint_is_still_read`, `damage_anywhere_is_detected`, `writing_replaces_the_checkpoint_in_one_step`, `overlapping_checkpoints_use_independent_temporary_files`, `concurrent_checkpoint_writes_publish_complete_files` | Checkpoint mã hóa rồi giải mã vẫn nguyên vẹn, dù có lệnh hay không, cùng với số tick và các giới hạn rủi ro của nó; một checkpoint cũ hơn được đọc lại với các giá trị mặc định; mọi bit bị lật và mọi lần cắt cụt đều bị phát hiện; việc ghi thay thế checkpoint cũ và bỏ qua tệp tạm còn sót lại; các lần ghi chồng lấp và đồng thời công bố checkpoint nguyên vẹn mà không dùng chung tệp tạm. |

Các kiểm thử sự cố của WAL mô phỏng trực tiếp nội dung tệp sau sự cố. Chúng tạo các
**crash image** (một phần đầu tệp hợp lệ, theo sau là những byte do ta chọn: phần đuôi
toàn số 0, một bản ghi bị ghi dở, nửa bản ghi) rồi kiểm tra các bộ đọc hiểu
chúng ra sao. Cách này tất định và nhanh, mà vẫn bao phủ những trạng thái mà một
sự cố dừng đột ngột có thể để lại.

### Doctests

`cargo test` cũng biên dịch và chạy đoạn mã nằm trong các chú thích tài liệu:

| Ở đâu | Kiểm tra gì |
|---|---|
| `README.md` (được nhúng vào `src/lib.rs`) | Phần hướng dẫn bắt đầu nhanh và các ví dụ API đều biên dịch và chạy được, nên README không thể lệch khỏi API. |
| Tài liệu mô-đun của `src/events.rs` | Ba loại event sink, đúng như chương 7 trình bày. |
| Tài liệu mô-đun của `src/ringbuffer.rs` | Một luồng producer và một consumer dừng khi bị ngắt kết nối, cộng thêm hai ví dụ `compile_fail`: nhân bản (clone) một `Producer`, và gọi `push` qua một `&Producer` dùng chung, đều **không** được biên dịch (mục 8.5). |

Doctest `compile_fail` là cách để kiểm thử một cam kết *ở mức kiểu* (type-level):
`cargo test` sẽ thất bại nếu đoạn mã bị cấm đó một ngày nào đó biên dịch được.

## 10.2 Kiểm thử đối chiếu (differential testing): `tests/model_check.rs`

Kiểm thử viết tay chỉ bao phủ những trường hợp tác giả nghĩ ra; lỗi thường xuất hiện khi kết hợp nhiều thao tác. Vì vậy, kiểm thử mạnh nhất của kho mã nguồn này so sánh bộ khớp lệnh với một sổ
lệnh thứ hai, **được cố tình giữ đơn giản**, trên những luồng yêu cầu ngẫu nhiên
rất dài, và yêu cầu hai bên cho kết quả giống nhau ở mọi dữ liệu có thể quan sát.

Sổ lệnh tham chiếu được viết để dễ kiểm tra tính đúng đắn, chứ không phải để chạy
nhanh:
một `BTreeMap` chứa các mức giá của mỗi phía, một `VecDeque` chứa
`(order_id, qty)` cho mỗi mức giá, một `HashMap` chứa các lệnh đang hoạt động và một
bộ đếm giao dịch. Mã đủ ngắn để rà soát trực tiếp.

Bộ sinh yêu cầu (`OpGenerator` trong [`tests/common/mod.rs`](../../tests/common/mod.rs))
dùng seed cố định, nên mỗi lần chạy đều tái lập được. Nó tạo ra:

- chủ yếu là các lệnh đặt trong một dải hẹp 16 tick, để các lệnh thường xuyên
  vượt spread của nhau; khoảng ba phần tư là lệnh giới hạn, phần còn lại là lệnh
  immediate-or-cancel, lệnh thị trường và lệnh post-only; mỗi lệnh đến từ một
  trong bốn tài khoản, và cứ năm lệnh thì có một lệnh yêu cầu chống tự giao dịch,
  nên các lệnh thường xuyên gặp lệnh của chính tài khoản mình;
- các lần hủy lệnh và giảm số lượng (cộng lại khoảng 30%), nhắm vào ID của lệnh đang hoạt động,
  ID đã khớp hết, ID chưa từng được cấp, và cả ID dành riêng; lần giảm đôi khi
  bằng 0 và thường bằng toàn bộ phần còn lại hoặc hơn;
- hiếm khi có lần thay đổi giới hạn rủi ro (khoảng một trên 500 yêu cầu): một
  số lượng tối đa, một biên độ giá, cả hai hoặc không có cái nào;
- thỉnh thoảng có lần hủy hàng loạt (khoảng một trên 200 yêu cầu), chủ yếu cho
  một tài khoản, đôi khi cho cả sổ lệnh;
- thỉnh thoảng có các đầu vào biên (edge input): số lượng bằng 0, tick ngoài
  phạm vi, ID dùng lại, các lệnh quét qua toàn bộ sổ, và số lượng quá lớn ở các
  tick rất xa khiến tổng của một mức giá bị tràn.

Sau **mỗi** yêu cầu, kiểm thử so sánh chính xác luồng sự kiện (kể cả trade ID), bid
tốt nhất và ask tốt nhất, số lệnh đang hoạt động, độ sâu tại mọi tick bộ sinh có thể
dùng, cùng `resting_order` của lệnh được yêu cầu và mọi maker đã giao dịch với
nó. Cứ mỗi 1,000 yêu cầu, kiểm thử còn so sánh toàn bộ `depth_snapshot`. Kiểm thử chạy
4 seed × 20,000 yêu cầu với arena nhỏ (64 lệnh, nên phần còn lại của lệnh thường
bị từ chối), thêm 4 seed với arena lớn (8,192 lệnh, nên sổ sâu hơn), tất cả đều
dùng 4096 tick mặc định; ngoài ra còn chạy 2 seed trên **thang giá 300,000 tick**
(bitset bốn tầng), nơi các lệnh quét và lệnh quá cỡ chạm tới cả hai đầu thang giá.

Hai chi tiết khiến kiểm thử này đáng tin:

- **Bộ đếm độ phủ (coverage counter).** Kiểm thử đếm số lần đi qua các nhánh quan
  trọng (đầu vào bị từ chối, phần còn lại bị từ chối, maker bị
  khớp một phần, hủy lệnh thành công, lời gọi tạo ra hơn 64 sự kiện, phần còn lại
  bị hết hạn, lệnh post-only bị từ chối, lần giảm số lượng, lần tự giao dịch bị
  ngăn chặn, lệnh bị gỡ bởi một lần hủy hàng loạt, lần từ chối do giới hạn rủi
  ro) và thất bại nếu có bộ đếm nào bằng 0. Nếu sửa bộ sinh khiến một nhánh không
  còn được chạy tới, thay đổi đó sẽ bị phát hiện.
- **Lỗi tái lập được.** Mọi thông báo assertion đều kèm seed, bước và yêu cầu,
  nên có thể phát lại chính xác trường hợp thất bại.

Đây là cách đáng tin cậy để kiểm tra một cấu trúc dữ liệu đã được tối ưu: triển
khai được tối ưu và triển khai tham chiếu dễ kiểm chứng không bao giờ được phép cho kết quả
khác nhau.

## 10.3 Tính tất định khi phát lại (replay): `tests/wal_replay.rs`

Kiểm thử này đưa 10,000 yêu cầu được sinh ra qua bộ khớp lệnh chính, đồng thời ghi từng yêu
cầu vào WAL *trước khi* áp dụng (dung lượng cấp phát trước nhỏ khiến tệp phải
mở rộng nhiều lần). Sau đó, kiểm thử hoàn tất nhật ký bằng `finalize`, đọc lại, kiểm tra dãy số thứ tự từ
0 đến 9,999 rồi phát lại các bản ghi vào bộ khớp lệnh **thứ hai**. Bản sao (replica)
này phải tạo ra đúng cùng các sự kiện (nên mỗi giao dịch có cùng
`trade_id`), cùng số lệnh đang hoạt động và cùng độ sâu.

Bản sao tự dựng bảng băm với **seed ngẫu nhiên** riêng. Vì vậy, kiểm thử này cũng
chứng minh điều đã nêu ở chương 5: seed chỉ thay đổi vị trí lưu phần tử, không
làm thay đổi hành vi của bộ khớp lệnh. `wal::replay` dựng bản sao theo dung lượng ghi
trong nhật ký, nên kiểm thử cũng xác nhận phần đầu tệp lưu thông tin đó.

`tests/snapshot.rs` kiểm tra điều tương tự với snapshot (mục 9.8). Kiểm thử chạy
10,000 yêu cầu được sinh ra, khôi phục bộ khớp lệnh thứ hai từ snapshot của bộ khớp lệnh thứ
nhất, rồi cho cả hai xử lý 10,000 yêu cầu tiếp theo. Mỗi lời gọi phải tạo ra
cùng các sự kiện, và cuối cùng hai sổ lệnh phải giống hệt nhau. Kiểm thử chạy một
lần với arena nhỏ và một lần với arena lớn.

## 10.4 Chứng minh "không cấp phát" (zero allocation)

Lời khẳng định *"`process_order`, `reduce_order` và `cancel_order` không bao giờ
cấp phát"* rất dễ viết trong chú thích nhưng cũng dễ vô tình phá vỡ về sau (chỉ
cần thêm một `Vec::new()` hay `.to_vec()` tưởng như vô hại). Vì vậy, điều này
được **kiểm thử** trong `tests/zero_alloc.rs`.

### Kỹ thuật: bộ cấp phát toàn cục có bộ đếm

Rust cho phép chương trình thay thế bộ cấp phát toàn cục (global allocator).
Kiểm thử cài một lớp bọc để đếm mọi lần cấp phát, rồi chuyển tiếp yêu cầu sang
bộ cấp phát của hệ thống:

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

`#[global_allocator]` ảnh hưởng đến **toàn bộ tệp thực thi kiểm thử**: mọi `Box`, `Vec` và
`String` ở bất kỳ đâu đều đi qua nó.

### Bản thân kiểm thử

```text
  1. Dựng engine thứ nhất với 5,000 ask có sẵn, và engine thứ hai  <- ở đây được phép cấp phát (chuẩn bị)
     với 500 ask một lot ở cùng một mức giá
  2. before = ALLOC_COUNT                                          <- chụp lại số lượng
  3. 5,000 lệnh mua nằm chờ (không khớp)                           <- phần việc được đo,
     5,000 lệnh mua có giá nằm trong dải ask                          qua một EventBatch dùng lại
     5,000 lần giảm số lượng đi một lot
     5,000 lần hủy lệnh
     một lần cancel_all mọi lệnh còn nằm chờ
     một taker quét hết 500 maker của engine thứ hai,                 qua một closure chỉ đếm
  4. after = ALLOC_COUNT
  5. assert_eq!(after, before) và thấy đủ 500 giao dịch            <- không có lần cấp phát nào ở giữa
```

Các điểm cần lưu ý khi bạn tự viết kiểm thử kiểu này:

- **Khởi động (warm up) và cấp phát sẵn trước**, để các lần cấp phát trong bước chuẩn bị
  không bị tính vào kết quả.
- **Chạy qua mọi đường đi** mà bạn muốn chứng minh là không cấp phát: nằm chờ,
  khớp lệnh, giảm số lượng, hủy lệnh, hủy hàng loạt, và một lần quét sâu vượt xa
  dung lượng của mọi bộ đệm cố định đều chạy các đoạn mã khác nhau.
- **Chọn các sink không cấp phát.** Vì mục tiêu là đo riêng bộ khớp lệnh, kiểm thử tái
  sử dụng một `EventBatch` (gọi `clear` trước mỗi lần) cùng một closure chỉ đếm.
- **`std::hint::black_box`** bọc quanh kết quả để trình tối ưu không xóa mất
  những lời gọi có kết quả không được dùng.
- **`realloc` cũng được tính:** `GlobalAlloc::realloc` mặc định gọi `alloc` của
  ta, nên việc một `Vec` tăng dung lượng cũng sẽ bị đếm.

Những gì nó **không** chứng minh:

- Bộ đếm ghi nhận mọi lần cấp phát **trong toàn bộ tiến trình**, từ tất cả các
  luồng. Vì vậy, phép đo chỉ chính xác khi đây là kiểm thử duy nhất trong tệp thực thi đó
  (tệp có ghi chú điều này), vì bộ chạy kiểm thử (test harness) chạy các kiểm thử song song trên nhiều
  luồng.
- Nó chỉ chạy các đường đi nêu trên, chứ không phải mọi đầu vào có thể (ví dụ
  các đường từ chối lệnh).
- `depth_snapshot` *có* cấp phát và cố tình bị loại ra.

Có thể chạy kiểm thử ở cả hai cấu hình; kiểm thử đều đạt:

```sh
cargo test --test zero_alloc
cargo test --release --test zero_alloc
```

## 10.5 Các cấu hình biên dịch (build profile)

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
  > **Thiết lập này chỉ có hiệu lực trong một tệp thực thi được biên dịch từ chính
  > package này.** `matching_engine` là một thư viện. Cargo bỏ qua các thiết lập
  > `[profile]` của dependency, nên một chương trình *dùng* crate này phải đặt
  > `panic = "abort"` trong `Cargo.toml` **của chính nó** thì mới có hành vi này.
  > Cargo cũng bỏ qua thiết lập `panic` cho kiểm thử và benchmark, vốn luôn unwind.
  > (Đó là lý do các kiểm thử như `double_free_is_detected` dùng được
  > `#[should_panic]`, và `panicking_sink_leaves_the_book_consistent` bắt được
  > một panic.)
- **`lto` và `codegen-units = 1`** cho phép trình biên dịch tối ưu xuyên qua
  ranh giới giữa các crate. Điều này quan trọng với các lời gọi từ benchmark hay
  từ chương trình của bạn vào thư viện (và từ thư viện vào `memmap2`).
  `#[inline(always)]` vốn đã hoạt động bên trong một crate mà không cần chúng.
- Để trình biên dịch dùng các lệnh quét bit mới nhất (`tzcnt`, `lzcnt`; xem
  [chương 4](04-price-ladder-bitset.md)), hãy biên dịch cho đúng CPU của bạn:

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench          # bash / zsh
```

```powershell
$env:RUSTFLAGS = "-C target-cpu=native"; cargo bench  # PowerShell
```

(Một tệp thực thi được biên dịch theo cách này có thể không chạy được trên các CPU cũ hơn.)

## 10.6 Các benchmark

[`benches/matching_engine_bench.rs`](../../benches/matching_engine_bench.rs) dùng
thư viện [criterion](https://docs.rs/criterion). Có bốn phép đo benchmark, mỗi phép đo chạy
trên một bộ khớp lệnh mới:

| Benchmark | Chuẩn bị (không tính giờ) | Phần việc được tính giờ | Số lệnh mỗi lần chạy |
|---|---|---|---|
| `place_10k_resting_orders` | bộ khớp lệnh rỗng, dung lượng 20,000 | 10,000 lệnh mua, mỗi lệnh ở tick `i % 4000`; không lệnh nào vượt spread nên lệnh nào cũng nằm chờ | 10,000 |
| `place_10k_resting_orders_300k_ticks` | như trên, với 300,000 tick | như trên, ở các tick `i × 29 % 300,000`, trải khắp toàn bộ thang giá | 10,000 |
| `cross_spread_matching` | 5,000 ask có số lượng 1 ở tick 100 | 5,000 lệnh mua có số lượng 1 ở tick 100; mỗi lệnh khớp với đúng một maker | 5,000 |
| `cancel_orders` | 10,000 lệnh mua đang chờ | hủy cả 10,000 lệnh theo ID | 10,000 |

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

- **`iter_batched`** dựng trạng thái mới cho mỗi lần chạy hàm được đo, nên thời gian
  đo không gồm `Engine::new` và mỗi lần chạy đều bắt đầu từ cùng một trạng thái.
  Việc trả `engine` ra khỏi hàm được đo khiến việc hủy nó diễn ra ngoài vùng tính
  giờ.
- **`black_box`** ngăn trình biên dịch tối ưu bỏ mất phần việc.
- Đầu tiên Criterion **chạy khởi động** (3 giây), rồi thu 100 mẫu và báo cáo một
  **khoảng tin cậy** (confidence interval): `time: [low estimate high]`. Giá trị
  "estimate" là ước lượng độ dốc hồi quy (regression slope) của Criterion cho
  **thời gian của một lần chạy hàm được đo**, nghĩa là cả lô 5,000 hoặc 10,000 lệnh,
  chứ không phải một lệnh.

Muốn có con số cho mỗi lệnh, hãy **chia cho số lệnh**.

### Một lần chạy mẫu (chỉ để tham khảo)

Đo trên chiếc máy dùng để viết các tài liệu này: Intel Core i9-12900H, Windows
11, Rust 1.98.1, `cargo bench`, chạy một lần, không cố giảm tải nền của máy.

| Benchmark | Thời gian báo cáo (estimate) | Số lệnh | ≈ mỗi lệnh |
|---|---|---|---|
| `place_10k_resting_orders` | 495 µs | 10,000 | ≈ 50 ns |
| `cross_spread_matching` | 346 µs | 5,000 | ≈ 69 ns |
| `cancel_orders` | 403 µs | 10,000 | ≈ 40 ns |

Phiên bản trước của chương này ghi nhận 746, 390 và 734 µs trên cùng chiếc máy
đó, với API cũ trả về một `EventBatch` 2 KiB mới cho mỗi lời gọi. Criterion
so sánh mỗi lần chạy với lần trước và báo các con số mới cải thiện khoảng 36%,
13% và 46%. Các thao tác ít tốn chi phí nhất được cải thiện nhiều nhất, đúng như dự đoán nếu việc
khởi tạo và sao chép batch đó chiếm phần đáng kể trong chi phí của chúng. Đây là
minh chứng bằng số liệu cho ý ở chương 7: "không cấp phát" không có nghĩa là
"không tốn chi phí". Vì chỉ có một lần chạy trên máy tính xách tay, hãy hiểu kết quả là
"nhanh hơn rõ rệt", đừng coi các tỷ lệ phần trăm là chính xác.

Khi thang giá được mở rộng để có số tầng bất kỳ (mục 4.6), một lần chạy trên cùng
chiếc máy đó cho kết quả:

| Benchmark | Thời gian báo cáo (estimate) | Số lệnh | ≈ mỗi lệnh |
|---|---|---|---|
| `place_10k_resting_orders` | 557 µs | 10,000 | ≈ 56 ns |
| `place_10k_resting_orders_300k_ticks` | 1.01 ms | 10,000 | ≈ 101 ns |
| `cross_spread_matching` | 381 µs | 5,000 | ≈ 76 ns |
| `cancel_orders` | 127 µs | 10,000 | ≈ 13 ns |

Có thể rút ra hai điều từ các con số này. Thứ nhất, Criterion đã so sánh bitset
tổng quát hóa với bitset hai tầng cố định (`cargo bench -- --save-baseline before`,
rồi `-- --baseline before`) và **không ghi nhận thay đổi nào** ở việc đặt lệnh và
khớp lệnh: với 4096 tick, vòng lặp chạy đúng hai bước như trước. Thứ hai, thang
giá rộng tốn thêm khoảng 45 ns cho mỗi lệnh, và có lẽ không phải vì hai lần quét
bit thêm: các lệnh được phân tán trên vùng mức giá chiếm 14 MB, nên phần lớn
không tìm thấy dữ liệu trong cache (cache miss). (`cancel_orders` cũng có vẻ
nhanh hơn trước nhiều, nhưng nhiều khả năng là nhờ benchmark chứ không phải nhờ
bộ khớp lệnh: `Engine` thu nhỏ từ 1,168 xuống 272 byte khi các bitset chuyển lên heap,
và bộ chạy benchmark của Criterion chuyển bộ khớp lệnh giữa bước chuẩn bị và hàm được đo.)

> [!IMPORTANT]
> Hãy coi các số này là ước lượng sơ bộ để hình dung bậc độ lớn, **không phải
> một lời hứa**.
> Phần cứng, hệ điều hành, thiết lập nguồn điện và tải nền của bạn đều làm các
> con số thay đổi, và không nên đem chúng so sánh giữa các máy.

### Cách hiểu kết quả đo

- Đây là **giá trị trung bình trên cả lô**, tức *thông lượng*. Chúng không nói gì
  về **đuôi**: 1 lệnh chậm nhất trong mỗi 1,000 lệnh. Với một hệ thống nhạy cảm về
  độ trễ, đuôi mới là thứ quan trọng nhất; muốn đo nó cần một bộ đếm giờ cho từng
  lời gọi, ghi kết quả vào một histogram (ví dụ crate `hdrhistogram`).
- Khối lượng công việc là **tổng hợp** (synthetic): các lệnh xoay vòng qua vài
  tick, tập dữ liệu đang dùng (working set) nhỏ và đã có sẵn trong cache, và không có
  mạng, không có WAL. Lưu lượng thực tế biến động và phức tạp hơn nhiều.
- Mỗi lời gọi ghi sự kiện của nó vào một `EventBatch` dùng lại. Phiên bản trước
  của bộ khớp lệnh thì trả về theo giá trị một batch 2 KiB mới từ mỗi lời gọi; API sink
  đã loại bỏ lần sao chép đó khỏi mọi lệnh (chương 7).
- Criterion được cấu hình với `default-features = false` (tính năng
  `html_reports` của nó bị tắt), nên nó chỉ in văn bản; dữ liệu thô vẫn được ghi
  dưới `target/criterion/<benchmark>/`.
- Criterion có thể cảnh báo *"Unable to complete 100 samples in 5.0s"*. Cảnh báo
  này vô hại: mỗi lần chạy hàm được đo xử lý nhiều thao tác (hàng nghìn lệnh), nên nó chỉ đơn giản là mất
  lâu hơn một chút.

## 10.7 Tích hợp liên tục (CI)

Mỗi lần push và pull request đều chạy
[`.github/workflows/ci.yml`](../../.github/workflows/ci.yml):

| Job | Chạy trên | Kiểm tra gì |
|---|---|---|
| **Kiểm thử** | `ubuntu-latest`, `windows-latest`, `ubuntu-24.04-arm`, Rust 1.98.1 | `cargo fmt --check`; `clippy -D warnings`; `cargo doc` với `-D warnings`; `cargo test` ở chế độ debug và release; các benchmark biên dịch được; các ví dụ `pipeline`, `walkthrough` và `checkpoint` chạy được |
| **MSRV** (phiên bản Rust tối thiểu được hỗ trợ) | `ubuntu-latest`, Rust 1.89 | `cargo check --lib`: cam kết `rust-version` trong `Cargo.toml` |
| **Miri** | `ubuntu-latest`, nightly | `cargo miri test --lib`: tìm hành vi không xác định (undefined behavior) trong mã `unsafe` (các kiểm thử có ánh xạ tệp thì bị bỏ qua) |

Mỗi lựa chọn nhằm phát hiện một loại lỗi cụ thể:

- **Windows** ánh xạ tệp vào bộ nhớ theo cách khác (nó từ chối thu nhỏ một tệp
  đang được ánh xạ), nên WAL được kiểm thử ở đó cũng như trên Linux.
- **ARM** có thể sắp xếp lại các truy cập bộ nhớ tự do hơn x86 rất nhiều. Một bộ
  đệm vòng thiếu `Acquire` thường vẫn vượt qua kiểm thử trên x86 mà có thể thất bại trên ARM, nên
  kiểm thử SPSC đa luồng cũng chạy trên phần cứng ARM thực tế. Cách đó vẫn chỉ mang tính
  xác suất; `loom` (chương 11) mới là kiểm tra vét cạn.
- **Một toolchain cố định phiên bản, một `Cargo.lock` được commit, và
  `--locked`** giúp CI tái lập được: một quy tắc kiểm tra (lint) mới của trình biên dịch hay một bản
  phát hành dependency mới không thể khiến quá trình biên dịch thất bại nếu không có thay đổi
  nào trong kho mã nguồn này. Việc nâng cấp trở thành những commit có chủ đích.
- **Timeout** khiến một kiểm thử bị treo thất bại sau vài phút, thay vì vài giờ.
- **Doctest của README và `tests/docs_sync.rs`** giữ cho tài liệu luôn khớp
  với mã nguồn: các ví dụ trong README và chương trình hoàn chỉnh ở chương 9 đều được
  biên dịch và chạy, nên chúng không thể lặng lẽ lỗi thời.

## 10.8 Những gì kiểm thử *không* bao phủ

Nhận biết các lỗ hổng cũng là một phần của việc đánh giá phạm vi kiểm thử.

| Lỗ hổng | Vì sao quan trọng | Cách lấp |
|---|---|---|
| **Lỗi về thứ tự bộ nhớ chỉ được phát hiện theo xác suất** | Job ARM chạy kiểm thử bộ đệm vòng đa luồng trên một CPU có thứ tự bộ nhớ yếu, và Miri kiểm tra mã `unsafe`, nhưng cả hai chỉ thấy những cách đan xen giữa các luồng tình cờ xảy ra. | Trình kiểm tra mô hình [`loom`](https://docs.rs/loom) vét cạn mọi cách đan xen (một bài tập ở chương 11). |
| **Sự cố thật và mất điện** | Các kiểm thử WAL dùng nội dung tệp sau sự cố được tạo thủ công (crash image); không có gì buộc tiến trình dừng giữa lúc đang ghi hay cắt điện. | Một bộ chạy kiểm thử cho bộ ghi chạy trong tiến trình con rồi buộc tiến trình này dừng ở những thời điểm ngẫu nhiên; kiểm thử mất điện cần một máy ảo hoặc một thiết bị khối có chèn lỗi (fault-injecting block device). |
| **Suy giảm hiệu năng (performance regression)** | CI chỉ *biên dịch* các benchmark, nên không tự phát hiện được hiệu năng giảm. | Baseline của Criterion (`cargo bench -- --save-baseline before`, về sau dùng `--baseline before`), hoặc một job benchmark chạy trên một máy chuyên dụng, ít tải. |
| **Các trường hợp biên của `depth_snapshot`** | `model_check` so sánh toàn bộ snapshot và một kiểm thử đơn vị kiểm tra `usize::MAX`, nhưng `levels = 0` và "ít mức giá hơn số mức hiện có" thì chưa được kiểm thử. | Hai kiểm thử đơn vị nhỏ. |
| **Hai bộ ghi trên cùng một tệp WAL** | Tài liệu ghi là không được hỗ trợ, nhưng không có gì thực thi hay kiểm thử điều đó. | Một khóa tệp (`File::try_lock`) và một kiểm thử cho nó (chương 11). |

## 10.9 Tiếp tục tối ưu hiệu năng

| Câu hỏi | Công cụ |
|---|---|
| Thời gian xử lý tập trung ở đâu? | Một profiler lấy mẫu (sampling profiler): `perf` + flamegraph trên Linux, profiler của Visual Studio hoặc `samply` trên Windows. |
| Trình biên dịch có thật sự sinh ra `tzcnt` không? | `cargo asm` / `cargo show-asm`, hoặc Godbolt. |
| Các đường đi khác có cấp phát ngầm không? | Counting allocator (bộ cấp phát có đếm) ở mục 10.4; `heaptrack` / DHAT. |
| Hành vi của cache có phải nút thắt cổ chai không? | Bộ đếm cache-miss của `perf stat`, `valgrind --tool=cachegrind`. |
| Các lần xử lý chậm nhất có độ trễ bao nhiêu? | Ghi thời lượng của từng lời gọi vào một HdrHistogram. |
| Bộ đệm vòng có thật sự đúng trên các mô hình bộ nhớ yếu không? | Job CI chạy trên ARM và Miri (cả hai đã có sẵn); `loom` để kiểm tra vét cạn. |

**Tiếp theo:** [11. Giới hạn và bài tập](11-limitations-and-exercises.md)
