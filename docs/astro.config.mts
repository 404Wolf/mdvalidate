import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";

import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  site: "https://404wolf.github.io",
  base: "/mdvalidate/",
  integrations: [
    starlight({
      title: "mdvalidate docs",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/404Wolf/mdvalidate",
        },
      ],
      customCss: ["./src/styles/global.css"],
      sidebar: [
        { label: "README.md", slug: "" },
        { label: "Getting started", slug: "getting-started" },
        {
          label: "Matchers",
          items: [
            { label: "Literals", slug: "matchers/01-literals" },
            { label: "Matchers", slug: "matchers/02-matchers" },
            { label: "Repetition and Lists", slug: "matchers/03-lists" },
            { label: "Code Blocks", slug: "matchers/04-code" },
            { label: "Tables", slug: "matchers/05-tables" },
            { label: "HTML", slug: "matchers/06-html" },
          ],
        },
        {
          label: "Misc",
          items: [
            { label: "Links and Images", slug: "misc/01-links" },
          ],
        },
      ],
    }),
  ],

  vite: {
    plugins: [tailwindcss()],
  },
});
