# C Example Project Makefile

CC = gcc
CFLAGS = -Wall -Wextra -I/usr/include
LDFLAGS = -lssl -lcrypto -lz -lcurl

TARGET = c_example
SRC = main.c

all: $(TARGET)

$(TARGET): $(SRC)
	$(CC) $(CFLAGS) -o $(TARGET) $(SRC) $(LDFLAGS)

clean:
	rm -f $(TARGET)

.PHONY: all clean
