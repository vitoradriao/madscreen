# Avisos de terceiros

O MadScreen é distribuído sob a **GNU GPL v3 ou posterior** (arquivo `LICENSE`). Este arquivo lista as licenças *declaradas*
pelas dependências; cada componente mantém sua própria licença e seus avisos. Gerado por
`node tools/release/third-party-notices.mjs`; não aprova patentes de codec nem substitui a revisão dos textos de licença.

## Componentes binários distribuídos separadamente

| Componente | Licença | Observação |
| --- | --- | --- |
| FFmpeg (libavcodec/libavutil/libswresample) | LGPL-2.1+ ou GPL (conforme a build) | A build fornecida inclui x264 (GPL); código-fonte correspondente e configuração acompanham cada release (`docs/runtime-distribution.md`). |
| x264 | GPL-2.0+ | Encoder CPU de fallback. Patentes AVC não são cobertas pela licença de copyright. |
| AMD AMF, Intel oneVPL | MIT | Dispatcher/headers; o encoder em si é do driver do fabricante. |
| NVIDIA Video Codec SDK (headers NVENC) | MIT | O encoder é do driver NVIDIA. |
| libwebrtc (via LiveKit/webrtc-sys) | BSD-3-Clause | Pré-compilado do projeto LiveKit. |
| LiveKit Rust SDK (`third_party/livekit`, `third_party/libwebrtc`, `third_party/webrtc-sys`) | Apache-2.0 | Cópia com modificações; ver os avisos nas pastas. |
| Cisco OpenH264 (opcional) | BSD-2-Clause | Somente o binário oficial da Cisco cobre royalties AVC; uma DLL compilada localmente não. |

## Crates Rust (Windows)

| Pacote | Versão | Licença |
| --- | --- | --- |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause |
| alloc-stdlib | 0.2.4 | BSD-3-Clause |
| anstream | 0.6.21 | MIT OR Apache-2.0 |
| anstyle | 1.0.14 | MIT OR Apache-2.0 |
| anstyle-parse | 0.2.7 | MIT OR Apache-2.0 |
| anstyle-query | 1.1.5 | MIT OR Apache-2.0 |
| anstyle-wincon | 3.0.11 | MIT OR Apache-2.0 |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| async-compression | 0.4.51 | MIT OR Apache-2.0 |
| async-trait | 0.1.92 | MIT OR Apache-2.0 |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT |
| autocfg | 1.5.1 | Apache-2.0 OR MIT |
| base64 | 0.21.7 | MIT OR Apache-2.0 |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| base64 | 0.23.1 | MIT OR Apache-2.0 |
| bit-set | 0.8.0 | Apache-2.0 OR MIT |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT |
| bitflags | 1.3.2 | MIT/Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| bmrng | 0.5.2 | MIT OR Apache-2.0 |
| brotli | 8.0.4 | BSD-3-Clause AND MIT |
| brotli-decompressor | 5.0.3 | BSD-3-Clause/MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| bytes | 1.12.1 | MIT |
| camino | 1.2.6 | MIT OR Apache-2.0 |
| cargo_metadata | 0.19.2 | MIT |
| cargo_toml | 0.22.3 | Apache-2.0 OR MIT |
| cargo-platform | 0.1.9 | MIT OR Apache-2.0 |
| cc | 1.10.0 | MIT OR Apache-2.0 |
| cfb | 0.7.3 | MIT |
| cfg_aliases | 0.2.2 | MIT |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| clap | 4.5.53 | MIT OR Apache-2.0 |
| clap_builder | 4.5.53 | MIT OR Apache-2.0 |
| clap_derive | 4.5.49 | MIT OR Apache-2.0 |
| clap_lex | 0.7.7 | MIT OR Apache-2.0 |
| codespan-reporting | 0.13.1 | Apache-2.0 |
| colorchoice | 1.0.5 | MIT OR Apache-2.0 |
| compression-codecs | 0.4.46 | MIT OR Apache-2.0 |
| compression-core | 0.4.34 | MIT OR Apache-2.0 |
| cookie | 0.18.2 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |
| crc32fast | 1.5.3 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| cssparser | 0.36.0 | MPL-2.0 |
| cssparser-macros | 0.6.1 | MPL-2.0 |
| csv | 1.4.0 | Unlicense/MIT |
| csv-core | 0.1.13 | Unlicense/MIT |
| ctor | 0.8.0 | Apache-2.0 OR MIT |
| ctor-proc-macro | 0.0.7 | Apache-2.0 OR MIT |
| ctrlc | 3.5.2 | MIT/Apache-2.0 |
| cxx | 1.0.198 | MIT OR Apache-2.0 |
| cxx-build | 1.0.198 | MIT OR Apache-2.0 |
| cxxbridge-flags | 1.0.198 | MIT OR Apache-2.0 |
| cxxbridge-macro | 1.0.198 | MIT OR Apache-2.0 |
| darling | 0.14.4 | MIT |
| darling | 0.21.3 | MIT |
| darling_core | 0.14.4 | MIT |
| darling_core | 0.21.3 | MIT |
| darling_macro | 0.14.4 | MIT |
| darling_macro | 0.21.3 | MIT |
| data-encoding | 2.11.1 | MIT |
| deranged | 0.5.9 | MIT OR Apache-2.0 |
| derive_more | 2.1.1 | MIT |
| derive_more-impl | 2.1.1 | MIT |
| device-info | 0.1.1 | Apache-2.0 |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| dirs | 6.0.0 | MIT OR Apache-2.0 |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 |
| dom_query | 0.27.0 | MIT |
| dpi | 0.1.2 | Apache-2.0 AND MIT |
| dtoa | 1.0.11 | MIT OR Apache-2.0 |
| dtoa-short | 0.3.5 | MPL-2.0 |
| dtor | 0.3.0 | Apache-2.0 OR MIT |
| dtor-proc-macro | 0.0.6 | Apache-2.0 OR MIT |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 |
| either | 1.19.0 | MIT OR Apache-2.0 |
| embed-resource | 3.0.12 | MIT |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| erased-serde | 0.4.11 | MIT OR Apache-2.0 |
| fastrand | 2.5.0 | Apache-2.0 OR MIT |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| fnv | 1.0.7 | Apache-2.0 / MIT |
| foldhash | 0.1.5 | Zlib |
| foldhash | 0.2.0 | Zlib |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |
| from_variants | 1.0.2 | MIT/Apache-2.0 |
| from_variants_impl | 1.0.2 | MIT/Apache-2.0 |
| fs2 | 0.4.3 | MIT/Apache-2.0 |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 |
| futures-core | 0.3.34 | MIT OR Apache-2.0 |
| futures-io | 0.3.34 | MIT OR Apache-2.0 |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 |
| futures-task | 0.3.34 | MIT OR Apache-2.0 |
| futures-util | 0.3.34 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| getrandom | 0.3.4 | MIT OR Apache-2.0 |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| glob | 0.3.4 | MIT OR Apache-2.0 |
| hashbrown | 0.12.3 | MIT OR Apache-2.0 |
| hashbrown | 0.15.5 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| hex | 0.4.3 | MIT OR Apache-2.0 |
| hmac | 0.12.1 | MIT OR Apache-2.0 |
| html5ever | 0.38.0 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| http-body | 1.1.0 | MIT |
| http-body-util | 0.1.5 | MIT |
| httparse | 1.10.1 | MIT OR Apache-2.0 |
| hyper | 1.12.0 | MIT |
| hyper-rustls | 0.27.10 | Apache-2.0 OR ISC OR MIT |
| hyper-tls | 0.6.0 | MIT/Apache-2.0 |
| hyper-util | 0.1.21 | MIT |
| ico | 0.5.0 | MIT |
| icu_collections | 2.1.1 | Unicode-3.0 |
| icu_locale_core | 2.1.1 | Unicode-3.0 |
| icu_normalizer | 2.1.1 | Unicode-3.0 |
| icu_normalizer_data | 2.1.1 | Unicode-3.0 |
| icu_properties | 2.1.2 | Unicode-3.0 |
| icu_properties_data | 2.1.2 | Unicode-3.0 |
| icu_provider | 2.1.1 | Unicode-3.0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 |
| idna | 1.1.0 | MIT OR Apache-2.0 |
| idna_adapter | 1.2.1 | Apache-2.0 OR MIT |
| indexmap | 1.9.3 | Apache-2.0 OR MIT |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| infer | 0.19.0 | MIT |
| ipnet | 2.12.2 | MIT OR Apache-2.0 |
| is_terminal_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| itertools | 0.14.0 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| jobserver | 0.1.35 | MIT OR Apache-2.0 |
| json-patch | 3.0.1 | MIT/Apache-2.0 |
| jsonptr | 0.6.3 | MIT OR Apache-2.0 |
| jsonwebtoken | 10.3.0 | MIT |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| libloading | 0.8.9 | ISC |
| link-cplusplus | 1.0.12 | MIT OR Apache-2.0 |
| litemap | 0.8.3 | Unicode-3.0 |
| livekit-api | 0.10.0 | Apache-2.0 |
| livekit-common | 0.1.6 | Apache-2.0 |
| livekit-data-stream | 0.2.0 | Apache-2.0 |
| livekit-datatrack | 0.2.4 | Apache-2.0 |
| livekit-net | 0.1.4 | Apache-2.0 |
| livekit-protocol | 0.8.2 | Apache-2.0 |
| livekit-region | 0.1.1 | Apache-2.0 |
| livekit-rpc | 0.2.0 | Apache-2.0 |
| livekit-signaling | 0.1.8 | Apache-2.0 |
| livekit-token | 0.4.0 | Apache-2.0 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| log | 0.4.34 | MIT OR Apache-2.0 |
| lru-slab | 0.1.3 | MIT OR Apache-2.0 OR Zlib |
| markup5ever | 0.38.0 | MIT OR Apache-2.0 |
| matchers | 0.2.0 | MIT |
| memchr | 2.8.3 | Unlicense OR MIT |
| mime | 0.3.17 | MIT OR Apache-2.0 |
| minisign-verify | 0.2.5 | MIT |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| mio | 1.2.4 | MIT |
| muda | 0.19.3 | Apache-2.0 OR MIT |
| multimap | 0.10.1 | MIT OR Apache-2.0 |
| native-tls | 0.2.18 | MIT OR Apache-2.0 |
| new_debug_unreachable | 1.0.6 | MIT |
| nu-ansi-term | 0.50.3 | MIT |
| num-conv | 0.1.0 | MIT OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| once_cell_polyfill | 1.70.2 | MIT OR Apache-2.0 |
| open | 5.4.5 | MIT |
| option-ext | 0.2.0 | MPL-2.0 |
| os_info | 3.15.0 | MIT |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| pbjson | 0.9.0 | MIT |
| pbjson-build | 0.9.0 | MIT |
| pbjson-types | 0.9.0 | MIT |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |
| petgraph | 0.8.3 | MIT OR Apache-2.0 |
| phf | 0.13.1 | MIT |
| phf_codegen | 0.13.1 | MIT |
| phf_generator | 0.13.1 | MIT |
| phf_macros | 0.13.1 | MIT |
| phf_shared | 0.13.1 | MIT |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 |
| plist | 1.8.0 | MIT |
| png | 0.17.16 | MIT OR Apache-2.0 |
| potential_utf | 0.1.6 | Unicode-3.0 |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 |
| precomputed-hash | 0.1.1 | MIT |
| prettyplease | 0.2.37 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| prost | 0.14.4 | Apache-2.0 |
| prost-build | 0.14.4 | Apache-2.0 |
| prost-derive | 0.14.4 | Apache-2.0 |
| prost-types | 0.14.4 | Apache-2.0 |
| quick-xml | 0.38.4 | MIT |
| quinn | 0.11.12 | MIT OR Apache-2.0 |
| quinn-proto | 0.11.19 | MIT OR Apache-2.0 |
| quinn-udp | 0.5.16 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| rand | 0.10.3 | MIT OR Apache-2.0 |
| rand | 0.9.5 | MIT OR Apache-2.0 |
| rand_chacha | 0.9.0 | MIT OR Apache-2.0 |
| rand_core | 0.10.1 | MIT OR Apache-2.0 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 |
| rand_core | 0.9.5 | MIT OR Apache-2.0 |
| rand_pcg | 0.10.2 | MIT OR Apache-2.0 |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| ref-cast | 1.0.27 | MIT OR Apache-2.0 |
| ref-cast-impl | 1.0.27 | MIT OR Apache-2.0 |
| regex | 1.13.1 | MIT OR Apache-2.0 |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| reqwest | 0.12.28 | MIT OR Apache-2.0 |
| reqwest | 0.13.5 | MIT OR Apache-2.0 |
| ring | 0.17.14 | Apache-2.0 AND ISC |
| rtrb | 0.3.6 | MIT OR Apache-2.0 |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT |
| rustls-native-certs | 0.8.5 | Apache-2.0 OR ISC OR MIT |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 |
| rustls-platform-verifier | 0.7.1 | MIT OR Apache-2.0 |
| rustls-webpki | 0.103.16 | ISC |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 |
| same-file | 1.0.6 | Unlicense/MIT |
| schannel | 0.1.30 | MIT |
| schemars | 0.8.22 | MIT |
| schemars | 0.9.0 | MIT |
| schemars | 1.2.3 | MIT |
| schemars_derive | 0.8.22 | MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| scratch | 1.0.9 | MIT OR Apache-2.0 |
| selectors | 0.36.1 | MPL-2.0 |
| semver | 1.0.28 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive_internals | 0.29.1 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 |
| serde_urlencoded | 0.7.1 | MIT/Apache-2.0 |
| serde_with | 3.17.0 | MIT OR Apache-2.0 |
| serde_with_macros | 3.17.0 | MIT OR Apache-2.0 |
| serde-untagged | 0.1.9 | MIT OR Apache-2.0 |
| serialize-to-javascript | 0.1.2 | MIT OR Apache-2.0 |
| serialize-to-javascript-impl | 0.1.2 | MIT OR Apache-2.0 |
| servo_arc | 0.4.3 | MIT OR Apache-2.0 |
| sha1 | 0.10.7 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| sharded-slab | 0.1.7 | MIT |
| shlex | 2.0.1 | MIT OR Apache-2.0 |
| signature | 2.2.0 | Apache-2.0 OR MIT |
| simd-adler32 | 0.3.10 | MIT |
| siphasher | 1.0.4 | MIT OR Apache-2.0 |
| slab | 0.4.12 | MIT |
| smallvec | 1.16.4 | MIT OR Apache-2.0 |
| socket2 | 0.6.5 | MIT OR Apache-2.0 |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 |
| string_cache | 0.9.0 | MIT OR Apache-2.0 |
| string_cache_codegen | 0.6.1 | MIT OR Apache-2.0 |
| strsim | 0.10.0 | MIT |
| strsim | 0.11.1 | MIT |
| subtle | 2.6.1 | BSD-3-Clause |
| syn | 1.0.109 | MIT OR Apache-2.0 |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| sync_wrapper | 1.0.2 | Apache-2.0 |
| synstructure | 0.14.0 | MIT |
| tao | 0.35.3 | Apache-2.0 |
| tauri | 2.11.6 | Apache-2.0 OR MIT |
| tauri-build | 2.6.3 | Apache-2.0 OR MIT |
| tauri-codegen | 2.6.3 | Apache-2.0 OR MIT |
| tauri-macros | 2.6.3 | Apache-2.0 OR MIT |
| tauri-plugin | 2.6.3 | Apache-2.0 OR MIT |
| tauri-plugin-deep-link | 2.4.10 | Apache-2.0 OR MIT |
| tauri-plugin-opener | 2.5.5 | Apache-2.0 OR MIT |
| tauri-plugin-process | 2.3.1 | Apache-2.0 OR MIT |
| tauri-plugin-single-instance | 2.4.5 | Apache-2.0 OR MIT |
| tauri-plugin-updater | 2.12.0 | Apache-2.0 OR MIT |
| tauri-runtime | 2.11.3 | Apache-2.0 OR MIT |
| tauri-runtime-wry | 2.11.4 | Apache-2.0 OR MIT |
| tauri-utils | 2.9.3 | Apache-2.0 OR MIT |
| tauri-winres | 0.3.7 | MIT |
| tempfile | 3.27.0 | MIT OR Apache-2.0 |
| tendril | 0.5.1 | MIT OR Apache-2.0 |
| termcolor | 1.4.1 | Unlicense OR MIT |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |
| thiserror | 2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |
| thread_local | 1.1.10 | MIT OR Apache-2.0 |
| time | 0.3.45 | MIT OR Apache-2.0 |
| time-core | 0.1.7 | MIT OR Apache-2.0 |
| time-macros | 0.2.25 | MIT OR Apache-2.0 |
| tinystr | 0.8.5 | Unicode-3.0 |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT |
| tokio | 1.54.1 | MIT |
| tokio-macros | 2.7.2 | MIT |
| tokio-native-tls | 0.3.1 | MIT |
| tokio-rustls | 0.26.6 | MIT OR Apache-2.0 |
| tokio-stream | 0.1.19 | MIT |
| tokio-tungstenite | 0.29.0 | MIT |
| tokio-util | 0.7.20 | MIT |
| toml | 0.9.12+spec-1.1.0 | MIT OR Apache-2.0 |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 0.7.5+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| tower | 0.5.3 | MIT |
| tower-http | 0.6.11 | MIT |
| tower-layer | 0.3.3 | MIT |
| tower-service | 0.3.3 | MIT |
| tracing | 0.1.44 | MIT |
| tracing-attributes | 0.1.31 | MIT |
| tracing-core | 0.1.36 | MIT |
| tracing-log | 0.2.0 | MIT |
| tracing-subscriber | 0.3.23 | MIT |
| tray-icon | 0.24.2 | MIT OR Apache-2.0 |
| try-lock | 0.2.5 | MIT |
| tungstenite | 0.29.0 | MIT OR Apache-2.0 |
| typed-path | 0.13.0 | MIT OR Apache-2.0 |
| typeid | 1.0.3 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unic-char-property | 0.9.0 | MIT/Apache-2.0 |
| unic-char-range | 0.9.0 | MIT/Apache-2.0 |
| unic-common | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-ident | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-version | 0.9.0 | MIT/Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 |
| unicode-width | 0.2.3 | MIT OR Apache-2.0 |
| untrusted | 0.9.0 | ISC |
| url | 2.5.8 | MIT OR Apache-2.0 |
| urlpattern | 0.3.0 | MIT |
| utf_types | 1.0.0 | Apache-2.0 OR MIT |
| utf8_iter | 1.1.0 | Apache-2.0 OR MIT |
| utf8parse | 0.2.2 | Apache-2.0 OR MIT |
| uuid | 1.26.1 | Apache-2.0 OR MIT |
| version_check | 0.9.5 | MIT/Apache-2.0 |
| vswhom | 0.1.0 | MIT |
| vswhom-sys | 0.1.3 | MIT |
| walkdir | 2.5.0 | Unlicense/MIT |
| want | 0.3.2 | MIT |
| web_atoms | 0.2.6 | MIT OR Apache-2.0 |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 |
| webrtc-sys-build | 0.3.21 | Apache-2.0 |
| webview2-com | 0.38.2 | MIT |
| webview2-com-macros | 0.8.1 | MIT |
| webview2-com-sys | 0.38.2 | MIT |
| winapi | 0.3.9 | MIT/Apache-2.0 |
| winapi-util | 0.1.11 | Unlicense OR MIT |
| window-vibrancy | 0.6.0 | Apache-2.0 OR MIT |
| windows | 0.61.3 | MIT OR Apache-2.0 |
| windows | 0.62.2 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.53.1 | MIT OR Apache-2.0 |
| windows-collections | 0.2.0 | MIT OR Apache-2.0 |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 |
| windows-core | 0.61.2 | MIT OR Apache-2.0 |
| windows-core | 0.62.2 | MIT OR Apache-2.0 |
| windows-future | 0.2.1 | MIT OR Apache-2.0 |
| windows-future | 0.3.2 | MIT OR Apache-2.0 |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 |
| windows-link | 0.1.3 | MIT OR Apache-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-numerics | 0.2.0 | MIT OR Apache-2.0 |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 |
| windows-registry | 0.5.3 | MIT OR Apache-2.0 |
| windows-registry | 0.6.1 | MIT OR Apache-2.0 |
| windows-result | 0.3.4 | MIT OR Apache-2.0 |
| windows-result | 0.4.1 | MIT OR Apache-2.0 |
| windows-strings | 0.4.2 | MIT OR Apache-2.0 |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 |
| windows-sys | 0.60.2 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows-targets | 0.53.5 | MIT OR Apache-2.0 |
| windows-threading | 0.1.0 | MIT OR Apache-2.0 |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 |
| windows-version | 0.1.7 | MIT OR Apache-2.0 |
| winnow | 0.7.15 | MIT |
| winnow | 1.0.4 | MIT |
| winreg | 0.56.0 | MIT |
| writeable | 0.6.4 | Unicode-3.0 |
| wry | 0.55.1 | Apache-2.0 OR MIT |
| yoke | 0.8.4 | Unicode-3.0 |
| yoke-derive | 0.8.4 | Unicode-3.0 |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerofrom | 0.1.8 | Unicode-3.0 |
| zerofrom-derive | 0.1.8 | Unicode-3.0 |
| zeroize | 1.9.1 | Apache-2.0 OR MIT |
| zerotrie | 0.2.5 | Unicode-3.0 |
| zerovec | 0.11.9 | Unicode-3.0 |
| zerovec-derive | 0.11.7 | Unicode-3.0 |
| zip | 4.6.1 | MIT |
| zip | 9.1.3 | MIT |
| zlib-rs | 0.6.8 | Zlib |
| zmij | 1.0.23 | MIT |

## npm: viewer

| Pacote | Versão | Licença | Dev |
| --- | --- | --- | --- |
| @bufbuild/protobuf | 1.10.1 | (Apache-2.0 AND BSD-3-Clause) |  |
| @esbuild/aix-ppc64 | 0.25.12 | MIT | sim |
| @esbuild/android-arm | 0.25.12 | MIT | sim |
| @esbuild/android-arm64 | 0.25.12 | MIT | sim |
| @esbuild/android-x64 | 0.25.12 | MIT | sim |
| @esbuild/darwin-arm64 | 0.25.12 | MIT | sim |
| @esbuild/darwin-x64 | 0.25.12 | MIT | sim |
| @esbuild/freebsd-arm64 | 0.25.12 | MIT | sim |
| @esbuild/freebsd-x64 | 0.25.12 | MIT | sim |
| @esbuild/linux-arm | 0.25.12 | MIT | sim |
| @esbuild/linux-arm64 | 0.25.12 | MIT | sim |
| @esbuild/linux-ia32 | 0.25.12 | MIT | sim |
| @esbuild/linux-loong64 | 0.25.12 | MIT | sim |
| @esbuild/linux-mips64el | 0.25.12 | MIT | sim |
| @esbuild/linux-ppc64 | 0.25.12 | MIT | sim |
| @esbuild/linux-riscv64 | 0.25.12 | MIT | sim |
| @esbuild/linux-s390x | 0.25.12 | MIT | sim |
| @esbuild/linux-x64 | 0.25.12 | MIT | sim |
| @esbuild/netbsd-arm64 | 0.25.12 | MIT | sim |
| @esbuild/netbsd-x64 | 0.25.12 | MIT | sim |
| @esbuild/openbsd-arm64 | 0.25.12 | MIT | sim |
| @esbuild/openbsd-x64 | 0.25.12 | MIT | sim |
| @esbuild/openharmony-arm64 | 0.25.12 | MIT | sim |
| @esbuild/sunos-x64 | 0.25.12 | MIT | sim |
| @esbuild/win32-arm64 | 0.25.12 | MIT | sim |
| @esbuild/win32-ia32 | 0.25.12 | MIT | sim |
| @esbuild/win32-x64 | 0.25.12 | MIT | sim |
| @livekit/mutex | 1.1.1 | Apache-2.0 |  |
| @livekit/protocol | 1.52.1 | Apache-2.0 |  |
| @types/dom-mediacapture-record | 1.0.22 | MIT |  |
| esbuild | 0.25.12 | MIT | sim |
| events | 3.3.0 | MIT |  |
| jose | 6.2.14 | MIT |  |
| livekit-client | 2.23.0 | Apache-2.0 |  |
| loglevel | 1.9.2 | MIT |  |
| machina | 7.0.1 | MIT |  |
| rxjs | 7.8.2 | Apache-2.0 |  |
| sdp | 3.2.2 | MIT |  |
| sdp-transform | 2.15.0 | MIT |  |
| tslib | 2.8.1 | 0BSD |  |
| typed-emitter | 2.1.0 | MIT |  |
| webrtc-adapter | 9.0.6 | BSD-3-Clause |  |

## npm: apps/madscreen-app

| Pacote | Versão | Licença | Dev |
| --- | --- | --- | --- |
| @esbuild/aix-ppc64 | 0.28.2 | MIT | sim |
| @esbuild/android-arm | 0.28.2 | MIT | sim |
| @esbuild/android-arm64 | 0.28.2 | MIT | sim |
| @esbuild/android-x64 | 0.28.2 | MIT | sim |
| @esbuild/darwin-arm64 | 0.28.2 | MIT | sim |
| @esbuild/darwin-x64 | 0.28.2 | MIT | sim |
| @esbuild/freebsd-arm64 | 0.28.2 | MIT | sim |
| @esbuild/freebsd-x64 | 0.28.2 | MIT | sim |
| @esbuild/linux-arm | 0.28.2 | MIT | sim |
| @esbuild/linux-arm64 | 0.28.2 | MIT | sim |
| @esbuild/linux-ia32 | 0.28.2 | MIT | sim |
| @esbuild/linux-loong64 | 0.28.2 | MIT | sim |
| @esbuild/linux-mips64el | 0.28.2 | MIT | sim |
| @esbuild/linux-ppc64 | 0.28.2 | MIT | sim |
| @esbuild/linux-riscv64 | 0.28.2 | MIT | sim |
| @esbuild/linux-s390x | 0.28.2 | MIT | sim |
| @esbuild/linux-x64 | 0.28.2 | MIT | sim |
| @esbuild/netbsd-arm64 | 0.28.2 | MIT | sim |
| @esbuild/netbsd-x64 | 0.28.2 | MIT | sim |
| @esbuild/openbsd-arm64 | 0.28.2 | MIT | sim |
| @esbuild/openbsd-x64 | 0.28.2 | MIT | sim |
| @esbuild/openharmony-arm64 | 0.28.2 | MIT | sim |
| @esbuild/sunos-x64 | 0.28.2 | MIT | sim |
| @esbuild/win32-arm64 | 0.28.2 | MIT | sim |
| @esbuild/win32-ia32 | 0.28.2 | MIT | sim |
| @esbuild/win32-x64 | 0.28.2 | MIT | sim |
| @napi-rs/lzma-linux-x64-gnu | 1.5.1 | MIT | sim |
| @rollup/rollup-android-arm-eabi | 4.66.0 | MIT | sim |
| @rollup/rollup-android-arm64 | 4.66.0 | MIT | sim |
| @rollup/rollup-darwin-arm64 | 4.66.0 | MIT | sim |
| @rollup/rollup-darwin-x64 | 4.66.0 | MIT | sim |
| @rollup/rollup-freebsd-arm64 | 4.66.0 | MIT | sim |
| @rollup/rollup-freebsd-x64 | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-arm-gnueabihf | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-arm-musleabihf | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-arm64-gnu | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-arm64-musl | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-loong64-gnu | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-loong64-musl | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-ppc64-gnu | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-ppc64-musl | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-riscv64-gnu | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-riscv64-musl | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-s390x-gnu | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-x64-gnu | 4.66.0 | MIT | sim |
| @rollup/rollup-linux-x64-musl | 4.66.0 | MIT | sim |
| @rollup/rollup-openbsd-x64 | 4.66.0 | MIT | sim |
| @rollup/rollup-openharmony-arm64 | 4.66.0 | MIT | sim |
| @rollup/rollup-win32-arm64-msvc | 4.66.0 | MIT | sim |
| @rollup/rollup-win32-ia32-msvc | 4.66.0 | MIT | sim |
| @rollup/rollup-win32-x64-gnu | 4.66.0 | MIT | sim |
| @rollup/rollup-win32-x64-msvc | 4.66.0 | MIT | sim |
| @tauri-apps/api | 2.11.1 | Apache-2.0 OR MIT |  |
| @tauri-apps/api | 2.12.4 | Apache-2.0 OR MIT |  |
| @tauri-apps/cli | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-darwin-arm64 | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-darwin-x64 | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-linux-arm-gnueabihf | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-linux-arm64-gnu | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-linux-arm64-musl | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-linux-riscv64-gnu | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-linux-x64-gnu | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-linux-x64-musl | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-win32-arm64-msvc | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-win32-ia32-msvc | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/cli-win32-x64-msvc | 2.11.5 | Apache-2.0 OR MIT | sim |
| @tauri-apps/plugin-deep-link | 2.4.10 | MIT OR Apache-2.0 |  |
| @tauri-apps/plugin-opener | 2.5.5 | MIT OR Apache-2.0 |  |
| @tauri-apps/plugin-process | 2.3.1 | MIT OR Apache-2.0 |  |
| @tauri-apps/plugin-updater | 2.12.0 | MIT OR Apache-2.0 |  |
| @types/estree | 1.0.9 | MIT | sim |
| esbuild | 0.28.2 | MIT | sim |
| fdir | 6.5.0 | MIT | sim |
| fsevents | 2.3.3 | MIT | sim |
| nanoid | 3.3.20 | MIT | sim |
| picocolors | 1.1.1 | ISC | sim |
| picomatch | 4.0.7 | MIT | sim |
| postcss | 8.5.32 | MIT | sim |
| rollup | 4.66.0 | MIT | sim |
| source-map-js | 1.2.2 | BSD-3-Clause | sim |
| tinyglobby | 0.2.17 | MIT | sim |
| typescript | 5.9.3 | Apache-2.0 | sim |
| vite | 7.3.7 | MIT | sim |

## npm: tools/cloudflare/worker

| Pacote | Versão | Licença | Dev |
| --- | --- | --- | --- |
| @cloudflare/kv-asset-handler | 0.5.0 | MIT OR Apache-2.0 | sim |
| @cloudflare/unenv-preset | 2.16.2 | MIT OR Apache-2.0 | sim |
| @cloudflare/workerd-darwin-64 | 1.20261023.1 | Apache-2.0 | sim |
| @cloudflare/workerd-darwin-arm64 | 1.20261023.1 | Apache-2.0 | sim |
| @cloudflare/workerd-linux-64 | 1.20261023.1 | Apache-2.0 | sim |
| @cloudflare/workerd-linux-arm64 | 1.20261023.1 | Apache-2.0 | sim |
| @cloudflare/workerd-windows-64 | 1.20261023.1 | Apache-2.0 | sim |
| @cloudflare/workers-types | 5.20261030.1 | MIT OR Apache-2.0 | sim |
| @cspotcode/source-map-support | 0.8.1 | MIT | sim |
| @emnapi/runtime | 1.11.3 | MIT | sim |
| @esbuild/aix-ppc64 | 0.25.12 | MIT | sim |
| @esbuild/aix-ppc64 | 0.28.2 | MIT | sim |
| @esbuild/android-arm | 0.25.12 | MIT | sim |
| @esbuild/android-arm | 0.28.2 | MIT | sim |
| @esbuild/android-arm64 | 0.25.12 | MIT | sim |
| @esbuild/android-arm64 | 0.28.2 | MIT | sim |
| @esbuild/android-x64 | 0.25.12 | MIT | sim |
| @esbuild/android-x64 | 0.28.2 | MIT | sim |
| @esbuild/darwin-arm64 | 0.25.12 | MIT | sim |
| @esbuild/darwin-arm64 | 0.28.2 | MIT | sim |
| @esbuild/darwin-x64 | 0.25.12 | MIT | sim |
| @esbuild/darwin-x64 | 0.28.2 | MIT | sim |
| @esbuild/freebsd-arm64 | 0.25.12 | MIT | sim |
| @esbuild/freebsd-arm64 | 0.28.2 | MIT | sim |
| @esbuild/freebsd-x64 | 0.25.12 | MIT | sim |
| @esbuild/freebsd-x64 | 0.28.2 | MIT | sim |
| @esbuild/linux-arm | 0.25.12 | MIT | sim |
| @esbuild/linux-arm | 0.28.2 | MIT | sim |
| @esbuild/linux-arm64 | 0.25.12 | MIT | sim |
| @esbuild/linux-arm64 | 0.28.2 | MIT | sim |
| @esbuild/linux-ia32 | 0.25.12 | MIT | sim |
| @esbuild/linux-ia32 | 0.28.2 | MIT | sim |
| @esbuild/linux-loong64 | 0.25.12 | MIT | sim |
| @esbuild/linux-loong64 | 0.28.2 | MIT | sim |
| @esbuild/linux-mips64el | 0.25.12 | MIT | sim |
| @esbuild/linux-mips64el | 0.28.2 | MIT | sim |
| @esbuild/linux-ppc64 | 0.25.12 | MIT | sim |
| @esbuild/linux-ppc64 | 0.28.2 | MIT | sim |
| @esbuild/linux-riscv64 | 0.25.12 | MIT | sim |
| @esbuild/linux-riscv64 | 0.28.2 | MIT | sim |
| @esbuild/linux-s390x | 0.25.12 | MIT | sim |
| @esbuild/linux-s390x | 0.28.2 | MIT | sim |
| @esbuild/linux-x64 | 0.25.12 | MIT | sim |
| @esbuild/linux-x64 | 0.28.2 | MIT | sim |
| @esbuild/netbsd-arm64 | 0.25.12 | MIT | sim |
| @esbuild/netbsd-arm64 | 0.28.2 | MIT | sim |
| @esbuild/netbsd-x64 | 0.25.12 | MIT | sim |
| @esbuild/netbsd-x64 | 0.28.2 | MIT | sim |
| @esbuild/openbsd-arm64 | 0.25.12 | MIT | sim |
| @esbuild/openbsd-arm64 | 0.28.2 | MIT | sim |
| @esbuild/openbsd-x64 | 0.25.12 | MIT | sim |
| @esbuild/openbsd-x64 | 0.28.2 | MIT | sim |
| @esbuild/openharmony-arm64 | 0.25.12 | MIT | sim |
| @esbuild/openharmony-arm64 | 0.28.2 | MIT | sim |
| @esbuild/sunos-x64 | 0.25.12 | MIT | sim |
| @esbuild/sunos-x64 | 0.28.2 | MIT | sim |
| @esbuild/win32-arm64 | 0.25.12 | MIT | sim |
| @esbuild/win32-arm64 | 0.28.2 | MIT | sim |
| @esbuild/win32-ia32 | 0.25.12 | MIT | sim |
| @esbuild/win32-ia32 | 0.28.2 | MIT | sim |
| @esbuild/win32-x64 | 0.25.12 | MIT | sim |
| @esbuild/win32-x64 | 0.28.2 | MIT | sim |
| @img/colour | 1.1.0 | MIT | sim |
| @img/sharp-darwin-arm64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-darwin-x64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-freebsd-wasm32 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-libvips-darwin-arm64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-darwin-x64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linux-arm | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linux-arm64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linux-ppc64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linux-riscv64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linux-s390x | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linux-x64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linuxmusl-arm64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-libvips-linuxmusl-x64 | 1.3.4 | LGPL-3.0-or-later | sim |
| @img/sharp-linux-arm | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-linux-arm64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-linux-ppc64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-linux-riscv64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-linux-s390x | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-linux-x64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-linuxmusl-arm64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-linuxmusl-x64 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-wasm32 | 0.35.5 | Apache-2.0 AND LGPL-3.0-or-later AND MIT | sim |
| @img/sharp-webcontainers-wasm32 | 0.35.5 | Apache-2.0 | sim |
| @img/sharp-win32-arm64 | 0.35.5 | Apache-2.0 AND LGPL-3.0-or-later | sim |
| @img/sharp-win32-ia32 | 0.35.5 | Apache-2.0 AND LGPL-3.0-or-later | sim |
| @img/sharp-win32-x64 | 0.35.5 | Apache-2.0 AND LGPL-3.0-or-later | sim |
| @jridgewell/resolve-uri | 3.1.2 | MIT | sim |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT | sim |
| @jridgewell/trace-mapping | 0.3.9 | MIT | sim |
| @poppinss/colors | 4.1.6 | MIT | sim |
| @poppinss/dumper | 0.6.5 | MIT | sim |
| @poppinss/exception | 1.2.3 | MIT | sim |
| @sindresorhus/is | 7.2.0 | MIT | sim |
| @speed-highlight/core | 1.2.24 | CC0-1.0 | sim |
| blake3-wasm | 2.1.5 | MIT | sim |
| cookie | 1.1.1 | MIT | sim |
| detect-libc | 2.1.2 | Apache-2.0 | sim |
| error-stack-parser-es | 1.0.5 | MIT | sim |
| esbuild | 0.25.12 | MIT | sim |
| esbuild | 0.28.2 | MIT | sim |
| fsevents | 2.3.3 | MIT | sim |
| kleur | 4.1.5 | MIT | sim |
| miniflare | 5.20261023.3-beta | MIT | sim |
| path-to-regexp | 6.3.0 | MIT | sim |
| pathe | 2.0.3 | MIT | sim |
| semver | 7.8.5 | ISC | sim |
| sharp | 0.35.5 | Apache-2.0 | sim |
| supports-color | 10.2.2 | MIT | sim |
| tslib | 2.8.1 | 0BSD | sim |
| typescript | 5.9.3 | Apache-2.0 | sim |
| undici | 7.30.1 | MIT | sim |
| unenv | 2.0.0-rc.24 | MIT | sim |
| workerd | 1.20261023.1 | Apache-2.0 | sim |
| wrangler | 4.156.0 | MIT OR Apache-2.0 | sim |
| ws | 8.21.0 | MIT | sim |
| youch | 4.1.0-beta.10 | MIT | sim |
| youch-core | 0.3.3 | MIT | sim |

