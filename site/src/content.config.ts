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

// API references for the PyPI/npm packages — the self-hosted analog to docs.rs
// for Rust. Generated markdown (committed, regenerated on release) so the site
// build stays node-only. See specs/documentation.md ("API reference hosting").
const apidocs = defineCollection({
  loader: glob({ pattern: "*.md", base: "./src/content/apidocs" }),
});

export const collections = { docs, rustdocs, apidocs };
