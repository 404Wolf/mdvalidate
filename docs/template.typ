// From https://github.com/talal/ilm/blob/main/template/main.typ

#let std-bibliography = bibliography
#let std-smallcaps = smallcaps
#let std-upper = upper

#let smallcaps(body) = std-smallcaps(text(tracking: 0.6pt, body))
#let upper(body) = std-upper(text(tracking: 0.6pt, body))

#let stroke-color = luma(200)
#let fill-color = luma(250)

#let template(
  title: [Your Title],
  author: "Author",
  paper-size: "a4",
  date: none,
  date-format: "[month repr:long] [day padding:zero], [year repr:full]",
  abstract: none,
  preface: none,
  table-of-contents: outline(),
  bibliography: none,
  chapter-pagebreak: true,
  external-link-circle: true,
  figure-index: (
    enabled: false,
    title: "",
  ),
  table-index: (
    enabled: false,
    title: "",
  ),
  listing-index: (
    enabled: false,
    title: "",
  ),
  body,
) = {
  set document(title: title, author: author)

  set text(size: 12pt)

  show raw: it => {
    if (it.lang == none) {
      it
    } else {
      set text(font: ("Iosevka", "Fira Mono"), size: 9pt)
      block(it, fill: rgb(250, 250, 250), radius: 0.3em, inset: 1.2em, width: 100%)
    }
  }

  set page(
    paper: paper-size,
    margin: (bottom: 1.75cm, top: 2.25cm),
  )

  page(
    align(
      left + horizon,
      block(width: 90%)[
        #v(-1.5in)

        #let v-space = v(2em, weak: true)
        #text(3em)[*#title*]

        #v-space
        #text(1.6em, author)

        #if abstract != none {
          v-space
          block(width: 80%)[
            // Default leading is 0.65em.
            #par(leading: 0.78em, justify: true, linebreaks: "optimized", abstract)
          ]
        }

        #if date != none {
          v-space
          text(date.display(date-format))
        }
      ],
    ),
  )

  set par(leading: 0.7em, spacing: 1.35em, justify: true, linebreaks: "optimized")

  show heading: it => {
    it
    v(2%, weak: true)
  }
  show heading: set text(hyphenate: false)

  show link: it => {
    it
    if external-link-circle and type(it.dest) != label {
      sym.wj
      h(1.6pt)
      sym.wj
      super(box(height: 3.8pt, circle(radius: 1.2pt, stroke: 0.7pt + rgb("#993333"))))
    }
  }

  if preface != none {
    page(preface)
  }

  if table-of-contents != none {
    table-of-contents
  }

  set page(
    footer: context {
      let i = counter(page).at(here()).first()

      let is-odd = calc.odd(i)
      let aln = if is-odd {
        right
      } else {
        left
      }

      let target = heading.where(level: 1)
      if query(target).any(it => it.location().page() == i) {
        return align(aln)[#i]
      }

      let before = query(target.before(here()))
      if before.len() > 0 {
        let current = before.last()
        let gap = 1.75em
        let chapter = upper(text(size: 0.68em, current.body))
        if current.numbering != none {
          if is-odd {
            align(aln)[#chapter #h(gap) #i]
          } else {
            align(aln)[#i #h(gap) #chapter]
          }
        }
      }
    },
  )

  set math.equation(numbering: "(1)")

  show raw.where(block: false): box.with(
    fill: fill-color.darken(2%),
    inset: (x: 3pt, y: 0pt),
    outset: (y: 3pt),
    radius: 2pt,
  )

  show raw.where(block: true): block.with(inset: (x: 5pt))

  show figure.where(kind: table): set block(breakable: true)
  set table(
    inset: 7pt,
    stroke: (0.5pt + stroke-color),
  )
  show table.cell.where(y: 0): smallcaps

  {
    set heading(numbering: "1.")

    show heading.where(level: 1): it => {
      if chapter-pagebreak {
        pagebreak(weak: true)
      }
      it
    }
    body
  }

  if bibliography != none {
    pagebreak()
    show std-bibliography: set text(0.85em)
    show std-bibliography: set par(leading: 0.65em, justify: false, linebreaks: auto)
    bibliography
  }

  let fig-t(kind) = figure.where(kind: kind)
  let has-fig(kind) = counter(fig-t(kind)).get().at(0) > 0
  if figure-index.enabled or table-index.enabled or listing-index.enabled {
    show outline: set heading(outlined: true)
    context {
      let imgs = figure-index.enabled and has-fig(image)
      let tbls = table-index.enabled and has-fig(table)
      let lsts = listing-index.enabled and has-fig(raw)
      if imgs or tbls or lsts {
        pagebreak()
      }

      if imgs {
        outline(
          title: figure-index.at("title", default: "Index of Figures"),
          target: fig-t(image),
        )
      }
      if tbls {
        outline(
          title: table-index.at("title", default: "Index of Tables"),
          target: fig-t(table),
        )
      }
      if lsts {
        outline(
          title: listing-index.at("title", default: "Index of Listings"),
          target: fig-t(raw),
        )
      }
    }
  }
}

#let blockquote(body) = {
  block(
    width: 100%,
    fill: fill-color,
    inset: 2em,
    stroke: (y: 0.5pt + stroke-color),
    body,
  )
}
