# Distribute the template matcher with the desktop app

The Building Template Matcher will be fine-tuned from the Apache-2.0 Chinese-CLIP RN50 model rather than trained from scratch or depend on a paid online embedding API. Training uses paired building photographs and template renders, including hard negatives that differ in material, window rhythm, period, or wall articulation.

The trained model is exported to ONNX and invoked directly by the desktop application through an embedded runtime. End users do not install Ollama, operate a model server, provide an API key, or understand the training stack. CPU inference is the compatibility baseline; supported hardware acceleration is an optimisation.

The model is an independently versioned optional application component downloaded on first use, then cached for permanent offline inference. This keeps the base installer small and permits matcher upgrades without an application release. Environments without direct network access may import the same signed model package manually.

Training and application inference remain separate concerns. Model releases are versioned, licensed, and evaluated against a held-out Chinese university building set before becoming the default matcher.

Training begins with maintainer-labelled examples and lawfully reusable public imagery. User photographs remain local by default and never become telemetry. After choosing a template, a user may separately opt in to contribute the clean crop and selected template under a confirmed reuse licence. The contribution pipeline removes EXIF metadata and masks identifiable faces and vehicle plates before upload.

The main application repository contains template rules, dataset manifests, training code, model version metadata, and traceable dataset references. Reviewed training crops and their licences live in a separately versioned dataset repository or dataset release so image growth, withdrawal, and re-review do not rewrite application history. Original photographs are not committed to Git. Signed ONNX model packages are published as releases for first-use download or offline import.

The proposal chooser includes “none match” as a valid outcome. With separate contribution consent, such a crop becomes an Uncovered Building Style Sample. It is retained for coverage analysis and possible future template authorship but never assigned to the nearest template as a positive training label.
