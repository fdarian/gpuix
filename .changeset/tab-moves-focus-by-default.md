---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Tab and Shift+Tab now move focus by default, like a browser. Any `onKeyDown` can cancel it.

The default runs after every element `onKeyDown` and the window `onKeyDown` of that keystroke. Key events now carry DOM-style controls:

```tsx
<div
  tabIndex={0}
  onKeyDown={(event) => {
    if (event.key !== 'tab') return
    event.preventDefault() // keep this Tab, for example to indent
    insertIndent()
  }}
/>
```

- `event.preventDefault()` cancels the default focus move
- `event.stopPropagation()` skips ancestor `onKeyDown` handlers and the window `onKeyDown`
- `event.defaultPrevented` reports an earlier `preventDefault()`
- `render(<App />, { tabNavigation: false })` turns the default off

Apps that called `renderer.focusNext()` from a window `onKeyDown` for Tab must remove that call, or Tab moves twice.

Tab no longer types a tab character into `<input>` or `<textarea>`.

Headless controls follow Base UI focus rules:

- `Select`, `Combobox`, and `Tooltip` triggers are tab stops, also with `asChild`. An explicit `tabIndex` on the part or on the child still wins
- An open Select popup keeps Tab inside, like Base UI's modal popup
- A press outside an open Select no longer pulls focus back to the trigger, so clicking into another input keeps focus there
- A Combobox closes when focus leaves its input, so a prevented Tab keeps it open
- Solid `asChild` now keeps the child's own event handlers and children
