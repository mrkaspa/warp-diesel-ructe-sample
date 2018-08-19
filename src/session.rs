use diesel::insert_into;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use models::User;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use rand::Rng;
use warp::filters::{cookie, BoxedFilter};
use warp::{self, reject, Filter};

type PooledPg = PooledConnection<ConnectionManager<PgConnection>>;
type PgPool = Pool<ConnectionManager<PgConnection>>;

/// A Session object is sent to most handler methods.
///
/// The content of the session object is application specific.
/// My session contains a session pool for the database and an
/// optional user (if logged in).
/// It may also contain pools to other backend servers (e.g. memcache,
/// redis, or application specific services) and/or other temporary
/// user data (e.g. a shopping cart in a web shop).
pub struct Session {
    db: PooledPg,
    user: Option<User>,
}

impl Session {
    /// Attempt to authenticate a user for this session.
    ///
    /// If the username and password is valid, create and return a session key.
    /// If authentication fails, simply return None.
    pub fn authenticate(
        &mut self,
        username: &str,
        password: &str,
    ) -> Option<String> {
        if let Some(user) = User::authenticate(self.db(), username, password)
        {
            info!("User {:?} authenticated", user);

            let secret = random_key(48);
            use schema::sessions::dsl::*;
            let result = insert_into(sessions)
                .values((user_id.eq(user.id), cookie.eq(&secret)))
                .execute(self.db());
            if Ok(1) == result {
                self.user = Some(user);
                return Some(secret);
            } else {
                error!(
                    "Failed to create session for {}: {:?}",
                    user.username, result,
                );
            }
        }
        None
    }

    pub fn from_key(db: PooledPg, sessionkey: Option<&str>) -> Self {
        use schema::sessions::dsl as s;
        use schema::users::dsl as u;
        let user = sessionkey.and_then(|sessionkey| {
            u::users
                .select((u::id, u::username, u::realname))
                .inner_join(s::sessions)
                .filter(s::cookie.eq(&sessionkey))
                .first::<User>(&db)
                .ok()
        });
        info!("Got: {:?}", user);
        Session { db, user }
    }
    pub fn user(&self) -> Option<&User> {
        self.user.as_ref()
    }
    pub fn db(&self) -> &PgConnection {
        &self.db
    }
}

fn random_key(len: usize) -> String {
    let mut rng = thread_rng();
    rng.sample_iter(&Alphanumeric).take(len).collect()
}

pub fn create_session_filter(db_url: &str) -> BoxedFilter<(Session,)> {
    let pool = pg_pool(db_url);
    warp::any()
        .and(cookie::optional("EXAUTH"))
        .and_then(move |key: Option<String>| {
            let pool = pool.clone();
            let key = key.as_ref().map(|s| &**s);
            match pool.get() {
                Ok(conn) => Ok(Session::from_key(conn, key)),
                Err(_) => {
                    error!("Failed to get a db connection");
                    Err(reject::server_error())
                }
            }
        }).boxed()
}

fn pg_pool(database_url: &str) -> PgPool {
    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Pool::new(manager).expect("Postgres connection pool could not be created")
}