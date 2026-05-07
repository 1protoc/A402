######## Test Client Makefile ########

CXX := g++
CXXFLAGS := -std=c++11 -Wall -g
INCLUDE := -I.. -I../Common -I../Common/network -I../Common/protocol

# Source files
TEST_CLIENT_SRC := test_client.cpp \
                   ../Common/network/message_serializer.cpp

# Object files
TEST_CLIENT_OBJ := $(TEST_CLIENT_SRC:.cpp=.o)

# Simple test
SIMPLE_TEST_SRC := test_simple.cpp
SIMPLE_TEST_OBJ := $(SIMPLE_TEST_SRC:.cpp=.o)

# Targets
.PHONY: all clean test_client test_simple

all: test_client test_simple

test_client: $(TEST_CLIENT_OBJ)
	@$(CXX) $(CXXFLAGS) -o test_client $(TEST_CLIENT_OBJ)
	@echo "Built test_client"

test_simple: $(SIMPLE_TEST_OBJ)
	@$(CXX) $(CXXFLAGS) -o test_simple $(SIMPLE_TEST_OBJ)
	@echo "Built test_simple"

%.o: %.cpp
	@$(CXX) $(CXXFLAGS) $(INCLUDE) -c $< -o $@
	@echo "CXX  <=  $<"

clean:
	@rm -f test_client test_simple $(TEST_CLIENT_OBJ) $(SIMPLE_TEST_OBJ)
	@rm -f ../Common/network/*.o
	@echo "Cleaned test files"

