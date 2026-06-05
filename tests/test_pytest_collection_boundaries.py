def test_root_pytest_stays_out_of_embedded_projects(pytestconfig):
    assert pytestconfig.getini("testpaths") == ["tests"]

    ignored_dirs = set(pytestconfig.getini("norecursedirs"))
    assert {"fixtures", "target"}.issubset(ignored_dirs)
