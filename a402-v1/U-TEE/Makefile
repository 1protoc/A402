######## AMD SEV-SNP Settings ########
# 注意：这是示例Makefile，实际需要根据AMD SEV-SNP SDK调整

SEV_SNP_SDK ?= /opt/amd/sev-snp-sdk
SEV_SNP_ARCH ?= x86_64

######## Common Source Files ########
COMMON_DIR := ../Common
COMMON_CRYPTO := $(COMMON_DIR)/crypto/adapter_signature.c \
                 $(COMMON_DIR)/crypto/encryption.c
COMMON_CHANNEL := $(COMMON_DIR)/channel/channel.c

######## Guest VM (Secure World) ########
GUEST_DIR := GuestVM
GUEST_SOURCES := $(GUEST_DIR)/utee_guest.cpp \
                 $(GUEST_DIR)/platform_sev.c \
                 $(GUEST_DIR)/sev_vm_communication.c \
                 $(COMMON_CRYPTO) \
                 $(COMMON_CHANNEL)

GUEST_INCLUDE := -I$(GUEST_DIR) \
                 -I$(COMMON_DIR)/crypto \
                 -I$(COMMON_DIR)/channel \
                 -I$(COMMON_DIR)/protocol \
                 -I$(COMMON_DIR)/storage \
                 -I$(COMMON_DIR)/utils \
                 -I$(COMMON_DIR)/vault \
                 -I$(COMMON_DIR)/mixer \
                 -I$(SEV_SNP_SDK)/include

GUEST_CFLAGS := -fPIC -Wall -m64 -std=c11 $(GUEST_INCLUDE)
GUEST_CXXFLAGS := -fPIC -Wall -m64 -std=c++11 $(GUEST_INCLUDE)
# GUEST_LDFLAGS := -L$(SEV_SNP_SDK)/lib -lsev_snp_guest -lcrypto -lssl
GUEST_LDFLAGS := -lcrypto -lssl

######## Host VM (Normal World) ########
HOST_DIR := HostVM
HOST_SOURCES := $(HOST_DIR)/host_vm_app.cpp \
                $(HOST_DIR)/utee_network.cpp \
                ../Common/HostVM/host_vm_mediator.cpp \
                ../Common/network/network_server.cpp \
                ../Common/network/message_serializer.cpp

HOST_INCLUDE := -I$(HOST_DIR) \
                -I$(GUEST_DIR) \
                -I../Common/HostVM \
                -I../Common/network \
                -I$(COMMON_DIR)/crypto \
                -I$(COMMON_DIR)/channel \
                -I$(COMMON_DIR)/protocol \
                -I$(COMMON_DIR)/utils \
                -I$(SEV_SNP_SDK)/include

HOST_CFLAGS := -fPIC -Wall -m64 $(HOST_INCLUDE)
HOST_CXXFLAGS := -fPIC -Wall -m64 -std=c++11 $(HOST_INCLUDE)
# HOST_LDFLAGS := -L$(SEV_SNP_SDK)/lib -lsev_snp_host -lpthread -lcrypto -lssl
HOST_LDFLAGS := -lpthread -lcrypto -lssl

######## Targets ########
.PHONY: all clean guest host

all: guest host

guest: $(GUEST_DIR)/utee_guest.bin
	@echo "Built SEV-SNP U-TEE Guest VM"

host: $(HOST_DIR)/utee_host_app
	@echo "Built SEV-SNP U-TEE Host VM App"

######## Guest VM Objects ########
$(GUEST_DIR)/utee_guest.o: $(GUEST_DIR)/utee_guest.cpp
	@$(CXX) $(GUEST_CXXFLAGS) -c $< -o $@
	@echo "CXX  <=  $<"

$(GUEST_DIR)/platform_sev.o: $(GUEST_DIR)/platform_sev.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(GUEST_DIR)/sev_vm_communication.o: $(GUEST_DIR)/sev_vm_communication.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/crypto/adapter_signature.o: $(COMMON_DIR)/crypto/adapter_signature.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/crypto/encryption.o: $(COMMON_DIR)/crypto/encryption.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/channel/channel.o: $(COMMON_DIR)/channel/channel.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/crypto/key_exchange.o: $(COMMON_DIR)/crypto/key_exchange.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/storage/response_storage.o: $(COMMON_DIR)/storage/response_storage.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/utils/logger.o: $(COMMON_DIR)/utils/logger.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/utils/timelock.o: $(COMMON_DIR)/utils/timelock.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/crypto/commitment.o: $(COMMON_DIR)/crypto/commitment.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/vault/vault.o: $(COMMON_DIR)/vault/vault.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(COMMON_DIR)/mixer/receipt.o: $(COMMON_DIR)/mixer/receipt.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

$(GUEST_DIR)/a402_utee.o: $(GUEST_DIR)/a402_utee.c
	@$(CC) $(GUEST_CFLAGS) -c $< -o $@
	@echo "CC   <=  $<"

######## Guest VM Binary ########
$(GUEST_DIR)/utee_guest.bin: $(GUEST_DIR)/utee_guest.o \
                              $(GUEST_DIR)/platform_sev.o \
                              $(GUEST_DIR)/sev_vm_communication.o \
                              $(COMMON_DIR)/crypto/adapter_signature.o \
                              $(COMMON_DIR)/crypto/encryption.o \
                              $(COMMON_DIR)/channel/channel.o
	@echo "Building SEV-SNP U-TEE Guest VM..."
	@echo "Note: Actual build commands depend on AMD SEV-SNP SDK"
	@echo "Guest VM needs to be built as a bootable image"
	@$(CXX) $(GUEST_CXXFLAGS) -o $@ $^ $(GUEST_LDFLAGS)
	# Additional steps: create SEV-SNP image, sign, etc.

######## Host VM Objects ########
$(HOST_DIR)/host_vm_app.o: $(HOST_DIR)/host_vm_app.cpp
	@$(CXX) $(HOST_CXXFLAGS) -c $< -o $@
	@echo "CXX  <=  $<"

$(HOST_DIR)/utee_network.o: $(HOST_DIR)/utee_network.cpp
	@$(CXX) $(HOST_CXXFLAGS) -c $< -o $@
	@echo "CXX  <=  $<"

../Common/HostVM/host_vm_mediator.o: ../Common/HostVM/host_vm_mediator.cpp
	@$(CXX) $(HOST_CXXFLAGS) -c $< -o $@
	@echo "CXX  <=  $<"

../Common/network/network_server.o: ../Common/network/network_server.cpp
	@$(CXX) $(HOST_CXXFLAGS) -c $< -o $@
	@echo "CXX  <=  $<"

../Common/network/message_serializer.o: ../Common/network/message_serializer.cpp
	@$(CXX) $(HOST_CXXFLAGS) -c $< -o $@
	@echo "CXX  <=  $<"

######## Host VM Executable ########
$(HOST_DIR)/utee_host_app: $(HOST_DIR)/host_vm_app.o \
                           $(HOST_DIR)/utee_network.o \
                           ../Common/HostVM/host_vm_mediator.o \
                           ../Common/network/network_server.o \
                           ../Common/network/message_serializer.o \
                           $(GUEST_DIR)/utee_guest.o \
                           $(GUEST_DIR)/platform_sev.o \
                           $(GUEST_DIR)/sev_vm_communication.o \
                           $(COMMON_DIR)/crypto/adapter_signature.o \
                           $(COMMON_DIR)/crypto/encryption.o \
                           $(COMMON_DIR)/crypto/key_exchange.o \
                           $(COMMON_DIR)/crypto/commitment.o \
                           $(COMMON_DIR)/storage/response_storage.o \
                           $(COMMON_DIR)/utils/logger.o \
                           $(COMMON_DIR)/utils/timelock.o \
                           $(COMMON_DIR)/vault/vault.o \
                           $(COMMON_DIR)/mixer/receipt.o \
                           $(COMMON_DIR)/channel/channel.o \
                           $(GUEST_DIR)/a402_utee.o
	@echo "Building SEV-SNP U-TEE Host VM App..."
	@echo "Note: Actual build commands depend on AMD SEV-SNP SDK"
	@$(CXX) $(HOST_CXXFLAGS) -o $@ $^ $(HOST_LDFLAGS)

clean:
	@rm -f $(GUEST_DIR)/*.bin $(GUEST_DIR)/*.o
	@rm -f $(HOST_DIR)/utee_host_app $(HOST_DIR)/*.o
	@rm -f ../Common/HostVM/*.o
	@rm -f ../Common/network/*.o
	@rm -f $(COMMON_DIR)/crypto/*.o $(COMMON_DIR)/channel/*.o
	@rm -f $(COMMON_DIR)/storage/*.o $(COMMON_DIR)/utils/*.o
	@rm -f $(COMMON_DIR)/vault/*.o $(COMMON_DIR)/mixer/*.o
	@echo "Cleaned SEV-SNP build files"
