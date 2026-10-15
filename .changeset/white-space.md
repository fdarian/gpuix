---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`whiteSpace` follows CSS. Spaces, tabs and newlines in a string collapse to one space (a newline is a space), across text nodes and span edges, and the white space at the start and end of a paragraph is removed. `pre`, `pre-wrap` and `pre-line` are new values, and `whiteSpace` is inherited. `getPaintedText()`, search, selection and copy read the processed text, and `getByText` and `findByText` compare with white space collapsed.

Breaking: text that relied on spaces or newlines being painted as written now collapses. Set `whiteSpace: 'pre'` (or `'pre-wrap'`) to keep them.
