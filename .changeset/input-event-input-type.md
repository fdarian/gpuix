---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

The `onChange` event of an `<input>` or `<textarea>` carries `inputType`, `data` and `isComposing` as a DOM `InputEvent` does in Chrome: `insertText`, `insertFromPaste`, `insertLineBreak`, `insertCompositionText`, `deleteCompositionText`, `deleteContentBackward` / `Forward`, `deleteWordBackward` / `Forward`, `deleteSoftLineBackward` / `Forward`, `deleteByCut`, `historyUndo` and `historyRedo`. `data` is the inserted text and `null` for an edit that inserted none; the number stepper has `inputType` `""`. The event is typed `HostInputEvent`. The `change` of a composition's last edit now comes before its `compositionend`, as in Chrome, and has `isComposing` true.
