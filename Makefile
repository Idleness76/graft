ollama_volume_name="ollama_data"

check_setup:
	make check_ollama_volume

check_ollama_volume:
		@if docker volume ls | grep -q ${ollama_volume_name} ; then \
                echo "Volume exists"; \
            else \
                make make_ollama_volume; \
            fi

make_ollama_volume:
	#create the volume from the base image
	docker create -v ${ollama_volume_name}:/data --name ${ollama_volume_name} alpine:latest

weavegraph:check_setup
	docker compose -f docker-compose.yml up -d --build --pull always
