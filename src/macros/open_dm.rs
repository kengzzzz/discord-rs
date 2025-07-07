#[macro_export]
macro_rules! open_dm {
    ($http:expr, $user:expr) => {{
        async {
            let resp = $http.create_private_channel($user).await?;
            resp.model().await.map_err(anyhow::Error::from)
        }
    }};
}
