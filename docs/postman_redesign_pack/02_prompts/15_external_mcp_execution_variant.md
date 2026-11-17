# 15 · Optional external coding-agent route

Use this with an agent connected to Figma's **remote MCP** and the current official Figma skills. Supply an actual file URL and local resource paths available to that agent. This is an execution wrapper for one of prompts 01–14, not a replacement for the spec.

```text
Target Figma Design file: [PASTE REAL FILE URL]
Target page/selection: [PAGE NAME OR REAL SELECTION LINK]
Task: [PASTE ONE NUMBERED TASK PROMPT]

Use the connected official Figma tools and load the required workflow skills before their corresponding operations. Inspect the target file and available libraries before editing. Use native write-to-canvas for editable frames, text, variables, components, variants and auto layout.

Apply the attached operating contract and specs. Read only the relevant context for this task. Do not turn a native-design task into an HTML screenshot or a Code Layer without an explicit reason. Keep imported reference assets on the Evidence page.

Execute in small dependent batches: inspect; ensure foundations/components; build the scoped screen/state group; read back structure; render/inspect the result; fix concrete issues. Keep output summaries compact and save exact file/node IDs for subsequent steps.

Verify native structure and visual output. Check component instance reuse, variable bindings/modes, text editability, sizing and overflow. A successful tool return is not enough to claim a usable design. If an action is unavailable, report the exact limitation and complete the parts that are supported.

One agent owns shared variables/main components. Any parallel screen-building agents work on separate named pages and may use shared instances but do not change the shared system. A separate reviewer may inspect without mutating.

For Figma Specs, check that the feature is available before writing a native Spec. If it is not, keep the provided Markdown as the brief instead; do not misrepresent ordinary text boxes as native Specs. For runnable behavior, use the approved Make/Code Layer route and preserve the native Design source.

Return real links to the created/changed frames, what was verified, unresolved issues and assumptions. Do not publish the library, share the file, send messages, or change unrelated files unless the user has authorized those actions.
```

## Official setup references

- [Remote MCP setup](https://developers.figma.com/docs/figma-mcp-server/remote-server-installation/)
- [Write to canvas](https://developers.figma.com/docs/figma-mcp-server/write-to-canvas/)
- [Figma skills for MCP](https://help.figma.com/hc/en-us/articles/39166810751895-Figma-skills-for-MCP)

Check the current plan, seat, client and tool availability when setting up. A connected read-only design integration is not proof that write-to-canvas is available.
