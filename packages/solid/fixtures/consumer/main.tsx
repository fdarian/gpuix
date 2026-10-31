import { createTestRoot } from "@ghia/solid/testing"
import { createView } from "./view.tsx"

const app = createTestRoot()
const fixture = createView()
app.render(fixture.view)
fixture.update()
await Promise.resolve()
console.log(app.renderer.getAllText().join(""))
app.unmount()
