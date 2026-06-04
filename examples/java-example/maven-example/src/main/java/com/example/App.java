package com.example;

import com.google.common.collect.ImmutableList;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.commons.lang3.StringUtils;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public class App {
    private static final Logger log = LoggerFactory.getLogger(App.class);

    public static void main(String[] args) throws Exception {
        log.info("Feluda Java Demo");

        ImmutableList<String> items = ImmutableList.of("license", "compliance", "scan");
        String joined = StringUtils.join(items, ", ");
        log.info("Items: {}", joined);

        ObjectMapper mapper = new ObjectMapper();
        String json = mapper.writeValueAsString(items);
        log.info("JSON: {}", json);
    }
}
