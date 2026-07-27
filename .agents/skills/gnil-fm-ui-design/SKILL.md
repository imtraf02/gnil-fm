---
name: gnil-fm-ui-design
description: >
  Dùng khi thêm, sửa hoặc redesign bất kỳ phần UI/UX nào trong gnil-fm (gnil-app crate), gồm
  component GPUI, panel/sidebar/list, dialog/sheet/menu, icon, layout và theme. Đọc trước khi viết
  view/render code. Bổ sung cho design-with-taste bằng semantic token, motion, component, Lucide SVG
  và theme JSON conventions cụ thể của project.
---

# UI/UX Design cho gnil-fm

Áp dụng các quy ước dưới đây trước khi viết GPUI view/component code trong `gnil-app`. Khi quy tắc
thẩm mỹ chung xung đột với quy ước project này, ưu tiên quy ước project.

## Nền tảng

- Chỉ dùng semantic token từ `theme_runtime`: `background`, `surface`, `surface_elevated`, `border`,
  `border_focused`, `text_muted`, `text`, `text_emphasized`, `accent`, `accent_background`,
  `accent_hover`, `danger`, `error`, `warning`, `git_added`, `git_modified`, `git_deleted`,
  `git_untracked`.
- Không hardcode `rgb(0x......)` trong component. Nếu cần khái niệm màu mới, thêm field vào
  `ThemeColors`, cập nhật built-in themes và JSON schema/fallback trước khi dùng.
- Giữ UI flat; không thêm gradient, glow, 3D, blur hoặc shadow nặng. Chỉ dùng `shadow_lg` mặc định
  cho overlay.
- Không thêm âm thanh, confetti hoặc hiệu ứng vui nhộn nếu không được yêu cầu rõ.

## Motion và overlay

- Dùng `render_appearance_menu` trong `crates/gnil-app/src/file_manager/view_appearance.rs` làm
  chuẩn.
- Mở trong 120ms bằng `ease_out_quint`, opacity 0→1 và dịch dọc 2–4px.
- Đóng trong 80ms bằng `quadratic`; luôn nhanh hơn mở.
- Với transition panel/page lớn, chỉ dùng 140–220ms. Với menu/tooltip, chỉ dùng 80–120ms.
- Luôn có nhánh `reduced_motion` trả element không animation.
- Dùng panel chặn propagation và backdrop riêng để click ngoài đóng. Escape phải đóng overlay/sheet.
- Dùng `anchored().position_mode(AnchoredPositionMode::Local).position(...).anchor(...).
  snap_to_window()` trong `deferred().with_priority(N)`. Kiểm tra priority hiện hữu trước khi thêm.

## Component

- Segmented control phải có stable `.id(("prefix", index))` cho từng lựa chọn.
- Selectable row có chiều cao chuẩn 36px khi chứa icon + label, `rounded_md`,
  `hover(bg(border()))`, và selected `bg(accent_background())`.
- Icon button nhỏ dùng hit target `size_6`, `rounded_md`, căn giữa; hover đổi cả nền lẫn màu icon.
- Mọi interactive element phải có default, hover, active/selected, disabled nếu áp dụng và focus.
- Giữ stable element identity khi list reorder hoặc re-render động để tránh nhấp nháy.

## Layout và typography

- Chỉ dùng spacing scale GPUI (`p_1`/`p_2`/`p_3`, `gap_1`/`gap_2`/`gap_3`,
  `size_3`/`size_6`/`size_7`). Chỉ dùng `px()` lệch thang khi có lý do layout cụ thể.
- Giữ menu/dropdown trong khoảng 240–320px. Sheet lớn phải có max-width cho cửa sổ hẹp.
- Dùng `rounded_md` mặc định; chỉ dùng `rounded_lg` cho panel/sheet cấp cao nhất.
- Dùng system font GPUI. Chỉ dùng monospace cho nội dung code/terminal thực sự.
- Dùng `text_xs` cho phụ, `text_sm` cho chính và `SEMIBOLD` cho section title.
- Dùng `text_muted`, `text`, `text_emphasized` theo phân cấp nội dung.
- Bọc text dài trong `.flex_1().min_w_0()` và thêm `.truncate()`.

## Icon và minh họa

- Lấy mọi icon chức năng, navigation, folder, file-kind, trash và device từ Lucide trước tiên.
- Pin chính xác một version Lucide trong `Cargo.toml`; không dùng wildcard hoặc range lỏng.
- Lưu icon chức năng dưới dạng SVG native với `viewBox="0 0 24 24"`, `stroke="currentColor"` và
  `stroke-width="2"`. Render bằng GPUI `svg()` để lấy màu semantic; không dùng raster.
- Giữ Lucide path gốc. Chỉ tự ghép khi Lucide không có composite tương đương.
- Với composite như folder-readonly/favorite, giữ thân Lucide và đặt badge nhất quán ở góc
  dưới-phải trong bounding box khoảng 6.5×5.5.
- Dùng compact tier ≤20px cho sidebar/list và detail tier ≥32px cho preview lớn.
- File icon chỉ biểu diễn kind: generic, code, text, image, document, archive, media.
- Folder icon chỉ biểu diễn state: closed, open, favorite, symlink, readonly.
- Empty-state illustration có thể bake màu nhưng chỉ dùng các sắc độ của hue sage; giữ SVG làm
  source of truth.

## Theme và accessibility

- Bảo đảm UI hoạt động với theme JSON partial override. Token thiếu phải kế thừa built-in palette
  theo appearance.
- Theme invalid không được chặn app khởi động; fallback built-in và giữ error count + Reload trong
  Appearance menu.
- Giữ contrast `text` và `text_emphasized` trên `background`/`surface` tối thiểu 4.5:1 ở Light và
  Dark.
- Đảm bảo icon phân biệt bằng shape ở 16px, không chỉ bằng màu.
- Giữ hit target tối thiểu 24×24px cho mọi nút.

## Quy trình cải tổ

1. Audit hardcoded color/font, overlay lệch pattern, icon lệch family/stroke và interactive state.
2. Giữ nguyên hành vi, keymap và protocol trừ khi yêu cầu nói rõ khác.
3. Tái dùng project primitives và cấu trúc Appearance menu trước khi tạo pattern mới.
4. Chia component/module theo trách nhiệm; không để một file render tích lũy quá nhiều màn hình.
5. Test Light/Dark, reduced motion bật/tắt và scale 100/150/200%.
6. Chạy `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` và
   `cargo test --workspace`.

## Không được làm

- Không hardcode component color hoặc font UI.
- Không thêm animation >220ms cho feedback thông thường.
- Không dùng icon raster, stroke khác 2 hoặc tự vẽ icon đã có trong Lucide.
- Không thêm gradient, glow, heavy shadow, sound hoặc confetti.
- Không thêm overlay thiếu reduced-motion, Escape hoặc backdrop dismiss.
- Không giả định user luôn dùng built-in theme.

## Definition of done

- Không còn component `rgb(0x...)` hoặc màu placeholder ad hoc ngoài theme modules.
- Mọi interactive element có hover/focus và disabled state khi áp dụng.
- Light/Dark, reduced-motion on/off và custom partial theme đều hoạt động.
- Icon rõ ở 16/20/24px, đúng Lucide/currentColor/stroke 2.
- Formatting, Clippy với warnings-as-errors và toàn bộ test đều pass.
