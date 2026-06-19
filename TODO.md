# Bondage Project Status

- [ ] `lookup` tool
	- [x] Local file lookup (anchor keyword search + radius context)
	- [x] Local directory lookup (listing folder items and recursive grep)
	- [x] Web URL scrap (fetch webpage text via HTTP)
	- [ ] Web search integration (querying search engine)
- [ ] `write` tool
	- [ ] Full file overwrite (creating missing folders automatically)
	- [ ] Substring match-and-replace patch (safety check for uniqueness)
- [ ] `bash` tool
	- [ ] Shell command execution
	- [ ] Output capture (binding stdout/stderr and exit status codes)
- [ ] GenAI Integration
	- [x] Use direct GenAI client structs without wrappers
	- [x] Implement conversion utilities in `util.rs`
	- [x] Implement standard `step` logic
	- [ ] Implement `step_stream` logic with text delta filtering
- [ ] Policies helper functions (managing allowances and security rules)
- [ ] Minimal CLI Harness (TBD)
