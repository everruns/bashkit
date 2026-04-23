// Decision: render user-facing guides from the canonical ../docs/ tree via
// Astro's glob loader so the site and the repo share a single source of truth.
import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";

const docs = defineCollection({
  loader: glob({ pattern: "*.md", base: "../docs" }),
});

export const collections = { docs };
