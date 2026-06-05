import importlib.util
import pathlib
import sys
import traceback


class Raises:
    def __init__(self, expected):
        self.expected = expected

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        if exc_type is None:
            raise AssertionError(f"expected {self.expected.__name__} to be raised")
        return issubclass(exc_type, self.expected)


def raises(expected):
    return Raises(expected)


def main():
    root = pathlib.Path.cwd()
    failures = []
    for test_file in sorted((root / "tests").glob("test_*.py")):
        spec = importlib.util.spec_from_file_location(test_file.stem, test_file)
        module = importlib.util.module_from_spec(spec)
        sys.modules["pytest"] = sys.modules[__name__]
        spec.loader.exec_module(module)
        for name in sorted(dir(module)):
            if name.startswith("test_"):
                try:
                    getattr(module, name)()
                    print(f"PASS {test_file.name}::{name}")
                except Exception:
                    failures.append(f"{test_file.name}::{name}")
                    traceback.print_exc()
    if failures:
        print(f"FAILED {len(failures)} tests: {failures}")
        return 1
    print("all tests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

