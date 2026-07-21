FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    OVERTURE_BRIDGE_HOST=0.0.0.0 \
    OVERTURE_BRIDGE_PORT=8765 \
    OVERTURE_CACHE_DIR=/var/cache/overture

WORKDIR /service
COPY overture-requirements.txt .
RUN pip install --no-cache-dir -r overture-requirements.txt
COPY overture_bridge.py .

EXPOSE 8765
VOLUME ["/var/cache/overture"]
CMD ["python", "overture_bridge.py"]
