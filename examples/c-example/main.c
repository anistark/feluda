#include <stdio.h>
#include <openssl/ssl.h>
#include <curl/curl.h>
#include <zlib.h>

int main() {
    printf("C example with transient dependencies\n");

    // Example usage of libraries with transient dependencies
    printf("OpenSSL version: %s\n", OPENSSL_VERSION_TEXT);

    curl_version_info_data *ver = curl_version_info(CURLVERSION_NOW);
    printf("libcurl version: %s\n", ver->version);

    printf("zlib version: %s\n", zlibVersion());

    return 0;
}
