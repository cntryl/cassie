#[test]
fn should_list_property_graphs_in_the_admin_catalog() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("admin session");
    for sql in [
        "CREATE TABLE rest_graph_people (id BIGINT PRIMARY KEY, name TEXT)",
        "CREATE TABLE rest_graph_follows (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, \
         b BIGINT NOT NULL)",
        "CREATE PROPERTY GRAPH rest_graph_social
           VERTEX TABLES (rest_graph_people AS person KEY (id) LABEL person PROPERTIES (name))
           EDGE TABLES (rest_graph_follows AS knows KEY (id)
             SOURCE KEY (a) REFERENCES person (id)
             DESTINATION KEY (b) REFERENCES person (id)
             LABEL knows PROPERTIES (id))",
    ] {
        cassie
            .execute_sql(&admin, sql, Vec::new())
            .expect("seed graph");
    }

    runtime.block_on(async {
        let (base_url, shutdown, server) = spawn_rest_server(cassie).await;
        let client = Client::new();
        let cookie = login_cookie(&client, &base_url).await;

        // Act
        let response = client
            .get(format!("{base_url}/api/v1/admin/catalog?database=postgres"))
            .header("cookie", &cookie)
            .send()
            .await
            .expect("catalog request");
        let status = response.status();
        let payload = response.json::<serde_json::Value>().await.expect("json");
        let graphs = section_items(&payload, "property_graphs");
        let elements = graphs[0]["columns"]
            .as_array()
            .expect("elements")
            .iter()
            .map(|element| {
                format!(
                    "{}: {}",
                    element["name"].as_str().expect("alias"),
                    element["data_type"].as_str().expect("description")
                )
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(status, StatusCode::OK);
        assert_eq!(graphs.len(), 1);
        assert_eq!(graphs[0]["kind"], "property_graph");
        assert_eq!(graphs[0]["name"], "rest_graph_social");
        assert_eq!(graphs[0]["schema"], "public");
        assert_eq!(graphs[0]["metadata"], "1 vertex table, 1 edge table");
        assert_eq!(
            elements,
            vec![
                "person: vertex on public.rest_graph_people",
                "knows: edge on public.rest_graph_follows"
            ]
        );

        stop_rest_server(shutdown, server).await;
    });
}
