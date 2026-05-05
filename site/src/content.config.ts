// Decision: render user-facing guides from their canonical markdown locations.
// Public articles live in ../docs; Rust API/integration guides stay in
// ../crates/bashkit/docs so rustdoc and the site share the same source files.
import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";

const docs = defineCollection({
  loader: glob({ pattern: "*.md", base: "../docs" }),
});

const rustdocs = defineCollection({
  loader: glob({ pattern: "*.md", base: "../crates/bashkit/docs" }),
});

export const collections = { docs, rustdocs };
