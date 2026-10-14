/** An ARIA boolean attribute, as React DOM accepts it. */
type AriaBoolean = boolean | "true" | "false"

/** A list of element `id`s separated by spaces. Each resolves by DOM `id`. */
type AriaIdList = string

/**
 * Every ARIA state and property the element accepts.
 *
 * Most map to an AccessKit node property. The ones that do not are accepted
 * and ignored: `aria-relevant`, `aria-grabbed`, `aria-dropeffect`.
 */
export interface AriaProps {
  // Naming and description
  "aria-label"?: string
  "aria-labelledby"?: AriaIdList
  "aria-description"?: string
  "aria-describedby"?: AriaIdList
  "aria-details"?: AriaIdList
  "aria-roledescription"?: string
  "aria-keyshortcuts"?: string
  "aria-braillelabel"?: string
  "aria-brailleroledescription"?: string
  "aria-placeholder"?: string

  // States
  "aria-checked"?: AriaBoolean | "mixed"
  "aria-pressed"?: AriaBoolean | "mixed"
  "aria-expanded"?: AriaBoolean
  "aria-selected"?: AriaBoolean
  "aria-disabled"?: AriaBoolean
  /** Removes the element and its subtree from the accessibility tree. */
  "aria-hidden"?: AriaBoolean
  "aria-invalid"?: AriaBoolean | "grammar" | "spelling"
  "aria-required"?: AriaBoolean
  "aria-readonly"?: AriaBoolean
  "aria-multiselectable"?: AriaBoolean
  /** Makes a `role="textbox"` multiline. */
  "aria-multiline"?: AriaBoolean
  "aria-modal"?: AriaBoolean
  "aria-busy"?: AriaBoolean
  "aria-current"?: AriaBoolean | "page" | "step" | "location" | "date" | "time"
  "aria-haspopup"?: AriaBoolean | "menu" | "listbox" | "tree" | "grid" | "dialog"
  "aria-orientation"?: "horizontal" | "vertical"
  "aria-sort"?: "none" | "ascending" | "descending" | "other"
  "aria-autocomplete"?: "none" | "inline" | "list" | "both"

  // Live regions
  "aria-live"?: "off" | "polite" | "assertive"
  "aria-atomic"?: AriaBoolean
  "aria-relevant"?: string

  // Values
  "aria-valuenow"?: number | string
  "aria-valuemin"?: number | string
  "aria-valuemax"?: number | string
  /** String value reported to assistive technology. */
  "aria-valuetext"?: string

  // Structure
  /** Heading level, 1-based. */
  "aria-level"?: number
  "aria-posinset"?: number
  "aria-setsize"?: number
  "aria-rowindex"?: number
  "aria-colindex"?: number
  "aria-rowcount"?: number
  "aria-colcount"?: number
  "aria-rowspan"?: number
  "aria-colspan"?: number
  "aria-rowindextext"?: string
  "aria-colindextext"?: string

  // Relationships
  "aria-controls"?: AriaIdList
  "aria-owns"?: AriaIdList
  "aria-flowto"?: AriaIdList
  "aria-errormessage"?: string
  /** On the container: the descendant `id` that has focus while the container does. */
  "aria-activedescendant"?: string
  "aria-grabbed"?: AriaBoolean
  "aria-dropeffect"?: string
}
