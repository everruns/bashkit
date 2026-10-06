// asciinema-player ships no type declarations; only `create` is used
// (see components/TerminalRecording.astro).
declare module "asciinema-player" {
  export function create(src: string, el: HTMLElement, opts?: Record<string, unknown>): unknown;
}
declare module "asciinema-player/dist/bundle/asciinema-player.css";
