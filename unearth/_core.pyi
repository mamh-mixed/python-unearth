class Link:
    url: str
    normalized: str
    comes_from: str | None
    yank_reason: str | None
    requires_python: str | None
    dist_metadata: dict[str, str] | bool | None

    def __init__(
        self,
        url: str,
        comes_from: str | None,
        yank_reason: str | None,
        requires_python: str | None,
        hashes: dict[str, str] | None,
        dist_metadata: dict[str, str] | bool | None,
    ) -> None: ...
    def __eq__(self, __value: object) -> bool: ...
    def __hash__(self) -> int: ...
    @property
    def url_without_fragment(self) -> str: ...
    @property
    def redacted(self) -> str: ...
    @property
    def is_file(self) -> bool: ...
    @property
    def file_path(self) -> str: ...
    @classmethod
    def from_path(cls, path: str) -> Link: ...
    @property
    def is_vcs(self) -> bool: ...
    @property
    def is_yanked(self) -> bool: ...
    @property
    def filename(self) -> str: ...
    @property
    def is_wheel(self) -> bool: ...
    @property
    def subdirectory(self) -> str | None: ...
    @property
    def hashes(self) -> dict[str, str] | None: ...
    @property
    def hash_options(self) -> dict[str, list[str]] | None: ...
    @property
    def dist_metadata_link(self) -> Link | None: ...

class Tag:
    interpreter: str
    abi: str
    platform: str

class TargetPython:
    def __init__(
        self,
        py_ver: tuple[int, int] | None,
        abis: list[str] | None,
        implementation: str | None,
        platforms: list[str] | None,
    ) -> None: ...
    @property
    def supported_tags(self) -> list[Tag]: ...
