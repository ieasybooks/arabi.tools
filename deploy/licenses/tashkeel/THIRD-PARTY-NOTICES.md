# Model metadata and notices

The library includes tokenizer mappings and numerical reference fixtures derived from the following upstream projects. Model weights are downloaded separately and retain their own upstream notices.

- [AliOsm/Shakkelha](https://github.com/AliOsm/shakkelha/tree/2214779456a905964d764dc4921fb70aa0d0aab6): character/diacritic mappings in `data/shakkelha.json` and corresponding reference fixtures. MIT; the original copyright and permission notice are in [licenses/Shakkelha-MIT](licenses/Shakkelha-MIT).
- [Abjad CATT](https://github.com/abjadai/catt/tree/8d5330499feb85f6625e6af632141ed2ed6065fd): character/diacritic mappings in `data/catt.json` and corresponding reference fixtures. Apache-2.0; the full license is in [licenses/CATT-Apache-2.0](licenses/CATT-Apache-2.0). The mappings have been represented as JSON and paired with preservation-aware Rust tokenization; the upstream runtime is not bundled.

See the repository's [model validation](https://github.com/ieasybooks/tashkeel-rs/blob/main/docs/model-validation.md) for checkpoint provenance and known numerical differences. The native ONNX Runtime library is a separate MIT-licensed dependency; executable release archives retain its license and third-party notices alongside generated Rust dependency notices.

## Models packaged by arabi.tools

The deployment also includes the Shakkelha normalized-big averaged-20 sigmoid ONNX conversion from [tashkeel-rs models-v1](https://github.com/ieasybooks/tashkeel-rs/releases/tag/models-v1), derived from the MIT-licensed Shakkelha checkpoint above. Tashkeel converted the checkpoint to ONNX and restored sigmoid recurrence gates; the pinned model is redistributed unchanged by arabi.tools. Its original copyright and license are retained in `licenses/Shakkelha-MIT`.

CATT's encoder and classifier are the unmodified files from the upstream [v2 eo_model_onnx.zip release](https://github.com/abjadai/catt/releases/tag/v2). The upstream project declares Apache-2.0; the archive contains no separate weight-specific notice. Its full license and Abjad AI copyright are retained in `licenses/CATT-Apache-2.0`.

These notices and licenses were copied from [tashkeel v0.1.0](https://github.com/ieasybooks/tashkeel-rs/tree/v0.1.0/crates/tashkeel), with this packaging section added by arabi.tools. The library's MIT license is retained in `LICENSE`. ONNX Runtime 1.31.0's license and third-party notices are installed separately under `/usr/share/doc/onnxruntime` in the container.
