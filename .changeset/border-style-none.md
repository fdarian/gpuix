---
'@gpuix/native': minor
---

An undeclared border style is `none`, the CSS initial value, and a `none` or `hidden` side has no width: nothing is painted and no space is taken. Before, a side no `border*` value gave a style was solid, so `{ borderWidth: 1, borderColor: 'red' }` drew a border. Now it needs `borderStyle: 'solid'` (or a side's `borderTopStyle`, or a shorthand with a style such as `border: '1px solid red'`), and only the sides that name a style have a border. A visible style with no width is `medium` (3px), as in CSS: `{ borderStyle: 'solid' }` is a 3px border. The same holds on inline `<span>` boxes, for the space they take along the line and for their paint, and in `hover` / `active` / `focus*` styles, which size their border from the style they sit on. A `border` shorthand without a style (`'2px red'`) keeps its width and colour for a later `borderStyle`. Add `borderStyle: 'solid'` wherever a width was set without a style.
