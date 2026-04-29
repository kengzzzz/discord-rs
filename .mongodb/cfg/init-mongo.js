print("Initializing the replica set...");
try {
  rs.initiate({
    _id: "rs0",
    members: [
      { _id: 0, host: "mongo:27017" },
    ]
  });
} catch (e) {
  print("Note: Replica set might be already initialized. " + e.message);
}

print("Waiting for election to complete...");
while (!rs.isMaster().ismaster) {
  sleep(1000);
}

print("Election complete, creating user root...");
var adminDB = db.getSiblingDB("admin");
try {
  adminDB.createUser({
    user: "homestead",
    pwd: "secret",
    roles: [{ role: "root", db: "admin" }]
  });
  print("Replica set initialized and user created successfully.");
} catch (e) {
  print("Note: User might already exist. " + e.message);
}