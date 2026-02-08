FROM python:3.12-slim

WORKDIR /app

# Install dependencies
COPY pyproject.toml uv.lock README.md ./
RUN pip install --no-cache-dir uv && uv sync --frozen

# Copy application
COPY src ./src

# Expose port
EXPOSE 4000

# Run API
CMD [".venv/bin/python", "-m", "uvicorn", "src.main:_get_app", "--factory", "--host", "0.0.0.0", "--port", "4000"]
