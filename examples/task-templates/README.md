# AO2 Task Templates

These templates are starting points for governed real-project runs. They keep
the same policy shape as the MVP risky PR workflow:

- deny by default;
- exact action digest approval;
- replay required;
- evidence cockpit required.

List embedded templates from an installed binary:

```sh
ao2 template list
```

Print a template:

```sh
ao2 template show bug-fix > bug-fix.yaml
ao2 run bug-fix.yaml --target /path/to/repo --provider codex --provider-prompt-file prompt.txt
```

The initial template set covers:

- `bug-fix`
- `small-refactor`
- `dependency-upgrade`
- `test-generation`
