# Welcome to `mdvalidate`!

MDS is a **tiny language for describing how Markdown *should look***. With `mdvalidate`! you write *schemas* that define a shape of Markdown, and MDS checks real documents against them.

It's designed for validating a stream of Markdown via stdin, so you can pipe input (like LLM output) and validate the shape of its response.

`mdvalidate`! schemas are normal markdown, that can consist of many "matcher" patterns, and all matchers have labels. This means that all validated markdown files can produce a JSON of matches found along the way.

Eventually `mdvalidate`! will support converting its schemas into a JSON schema describing the shape of the output that it produces once it has validated some Markdown file.

`mdvalidate`! is written in 100% safe rust and is 🔥 blazingly fast 🔥.

# `demo-title:/.*/`

- first
- `item:/shallow\d/`{2,2}
  - `item:/deep\d/`{,}

By `name:/[A-Za-z]+/`

### Codeblocks

```{lang:/.*/}
{content}
