/* tslint:disable */
/* eslint-disable */
/**
*/
export class WebMobiPlayer {
  free(): void;
/**
* @returns {number}
*/
  get_height(): number;
/**
* @param {number} target_frame
* @returns {boolean}
*/
  seek_frame(target_frame: number): boolean;
/**
* @returns {number}
*/
  get_channels(): number;
/**
* @returns {number}
*/
  get_rgba_ptr(): number;
/**
* @returns {number}
*/
  get_rgba_size(): number;
/**
* @returns {number}
*/
  get_sample_rate(): number;
/**
* @returns {Float32Array}
*/
  decode_all_audio(): Float32Array;
/**
* @returns {number}
*/
  get_total_frames(): number;
/**
* @returns {boolean}
*/
  decode_next_frame(): boolean;
/**
* @returns {number}
*/
  get_current_frame(): number;
/**
*/
  constructor();
/**
* @returns {number}
*/
  get_fps(): number;
/**
* @returns {number}
*/
  get_width(): number;
/**
* @returns {boolean}
*/
  has_audio(): boolean;
/**
* @param {Uint8Array} data
* @returns {boolean}
*/
  load_file(data: Uint8Array): boolean;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly __wbg_webmobiplayer_free: (a: number) => void;
  readonly webmobiplayer_decode_all_audio: (a: number, b: number) => void;
  readonly webmobiplayer_decode_next_frame: (a: number, b: number) => void;
  readonly webmobiplayer_get_channels: (a: number) => number;
  readonly webmobiplayer_get_current_frame: (a: number) => number;
  readonly webmobiplayer_get_fps: (a: number) => number;
  readonly webmobiplayer_get_height: (a: number) => number;
  readonly webmobiplayer_get_rgba_ptr: (a: number) => number;
  readonly webmobiplayer_get_rgba_size: (a: number) => number;
  readonly webmobiplayer_get_sample_rate: (a: number) => number;
  readonly webmobiplayer_get_total_frames: (a: number) => number;
  readonly webmobiplayer_get_width: (a: number) => number;
  readonly webmobiplayer_has_audio: (a: number) => number;
  readonly webmobiplayer_load_file: (a: number, b: number, c: number, d: number) => void;
  readonly webmobiplayer_new: () => number;
  readonly webmobiplayer_seek_frame: (a: number, b: number) => number;
  readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {SyncInitInput} module
*
* @returns {InitOutput}
*/
export function initSync(module: SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {InitInput | Promise<InitInput>} module_or_path
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: InitInput | Promise<InitInput>): Promise<InitOutput>;
