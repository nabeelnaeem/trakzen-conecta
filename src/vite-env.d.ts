/// <reference types="vite/client" />

declare module "*.wav?inline" {
  const src: string;
  export default src;
}

declare const __APP_VERSION__: string;
declare const __BUILD_DATE__: string;
declare const __BUILD_ID__: string;
