# AGENTS.md

This file records repository-specific maintenance preferences for human contributors and coding agents.

本文件记录仓库级的维护偏好，供人工维护者和代码代理共同遵守。

## Scope

These rules apply to the whole `modelUsage` repository unless a future subdirectory adds a more specific `AGENTS.md`.

这些规则适用于整个 `modelUsage` 仓库；除非未来某个子目录新增了更具体的 `AGENTS.md`。

## Documentation layout

- User-facing documentation should live in `README.md` and `README-zh.md`.
- Pricing maintenance rules should live in `PRICING.md` and `PRICING-zh.md`.
- Repository collaboration preferences should live in `AGENTS.md`, not in the README.

- 面向用户的说明放在 `README.md` 和 `README-zh.md`。
- 价格维护规则放在 `PRICING.md` 和 `PRICING-zh.md`。
- 仓库协作偏好放在 `AGENTS.md`，不要塞进 README。

## Language conventions

- Source-code comments should be bilingual when they describe non-trivial logic.
- The order must be English first, then Chinese.
- Short obvious code does not need comments just to satisfy the rule.

- 代码里的关键注释使用双语。
- 顺序必须是英文在前，中文在后。
- 很短且显而易见的代码不用为了凑规则强行加注释。

## Commit message conventions

- Commit messages should be bilingual when the change is substantial.
- Write the English block first.
- Write the Chinese block after the English block.
- Do not alternate sentence by sentence between English and Chinese.

- 变更较大时，提交信息使用双语。
- 先写英文整段。
- 再写中文整段。
- 不要中英文一句一句交替写。

## README conventions

- Keep `README.md` as the English primary document.
- Keep `README-zh.md` as the Chinese counterpart.
- If installation, platform support, versioning, or release flow changes, update both files together.

- `README.md` 作为英文主文档。
- `README-zh.md` 作为中文对应文档。
- 只要安装方式、平台支持、版本规则、发版流程有变化，就要同时更新两份文档。

## Pricing conventions

- Pricing is maintained locally in `pricing/official-pricing.json`.
- Do not rely on LiteLLM as the repository source of truth.
- Verify new prices from official published sources before updating the pricing file.
- If a model price is unknown, keep it as `N/A`; do not silently convert it to zero.

- 价格以本地的 `pricing/official-pricing.json` 为准。
- 不要把 LiteLLM 当作仓库里的权威价格来源。
- 新模型价格必须先从官方公开来源核对，再更新价格文件。
- 如果模型价格未知，就保持 `N/A`，不要偷偷算成 0。

## Model normalization rules

- Claude model normalization may strip date suffixes, but should keep real model versions.
- Codex/OpenAI model normalization should strip provider prefixes only.
- Do not merge Codex minor versions such as `gpt-5.2`, `gpt-5.3-codex`, or `gpt-5.1-codex-max`.

- Claude 模型归一化可以去日期后缀，但要保留真实版本。
- Codex/OpenAI 模型归一化只去 provider 前缀。
- 不要合并 `gpt-5.2`、`gpt-5.3-codex`、`gpt-5.1-codex-max` 这类 Codex 小版本。

## Reporting conventions

- The default report should stay focused on the latest 30 days unless there is a strong reason to change it.
- `--all` should remain the explicit way to show full history.
- Codex-only reports should follow `ccusage-codex` display semantics as closely as practical.

- 默认报表应继续聚焦最近 30 天，除非有很强理由再改。
- `--all` 继续作为查看全量历史的显式开关。
- Codex-only 报表应尽量对齐 `ccusage-codex` 的展示口径。

## Platform conventions

- The project currently documents Linux only.
- If cross-platform support is added, update both README files and release workflows together.

- 当前项目文档只承诺支持 Linux。
- 如果以后新增跨平台支持，要同时更新中英文 README 和 release workflow。

## Release conventions

- The crate version in `Cargo.toml` is the source of truth for the project version.
- Git tags should use the `vX.Y.Z` format.
- GitHub Actions should continue to build Linux release artifacts from version tags.

- `Cargo.toml` 里的版本号是项目版本的唯一来源。
- Git tag 使用 `vX.Y.Z` 格式。
- GitHub Actions 应继续在版本 tag 上构建 Linux release 产物。

## When changing behavior

- If a behavior change affects report semantics, update tests if possible.
- If a behavior change affects displayed columns or field meanings, update both README files.
- If a behavior change affects pricing rules, update both PRICING files.

- 如果行为改动影响报表语义，尽量同步更新测试。
- 如果行为改动影响列含义或展示方式，要同步更新两份 README。
- 如果行为改动影响价格规则，要同步更新两份 PRICING 文档。
