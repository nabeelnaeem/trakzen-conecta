/// <reference types="vite/client" />

declare module "*.wav?inline" {
  const src: string;
  export default src;
}
