# Databend Python Binding

This crate intends to build a native python binding.

## Installation

```bash
pip install databend
```

## Usage

### Basic:

```python
from databend import SessionContext

ctx = SessionContext()

df = ctx.sql("select number, number + 1, number::String as number_p_1 from numbers(8)")

# convert to pyarrow
df.to_py_arrow()

# convert to pandas
df.to_pandas()

```

### Tenant separation:

```python
ctx = SessionContext(tenant = "a")
```


## Development

Setup virtualenv:

```shell
python -m venv .venv
```

Activate venv:

```shell
source .venv/bin/activate
````

Install `maturin`:

```shell
pip install "maturin[patchelf]"
```

Build bindings:

```shell
maturin develop
```

Run tests:

```shell
maturin develop -E test
```

Build API docs:

```shell
maturin develop -E docs
pdoc opendal
```

## Storage configuration

- Meta Storage directory(Catalogs, Databases, Tables, Partitions, etc.): `./.databend/_meta`
- Data Storage directory: `./.databend/_data`
- Cache Storage directory: `./.databend/_cache`
- Logs directory: `./.databend/logs`

## More

Databend python api is inspired by [arrow-datafusion-python](https://github.com/apache/arrow-datafusion-python), thanks for their great work.