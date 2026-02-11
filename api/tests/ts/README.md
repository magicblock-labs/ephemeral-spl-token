uv run uvicorn src.main:create_app --factory --reload

API_URL=http://localhost:8000 yarn test
