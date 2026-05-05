#!/usr/bin/env bash
set -euo pipefail

mongo_host="mongo:27017"

echo "Initializing the replica set..."
if ! mongosh --host "$mongo_host" --quiet --eval '
try {
  rs.initiate({
    _id: "rs0",
    members: [{ _id: 0, host: "mongo:27017" }]
  });
} catch (e) {
  print("Note: Replica set might be already initialized. " + e.message);
}
'; then
  echo "Failed to initialize replica set"
  exit 1
fi

echo "Waiting for election to complete..."
until [ "$(mongosh --host "$mongo_host" --quiet --eval 'rs.isMaster().ismaster ? "true" : "false"')" = "true" ]; do
  sleep 1
done

echo "Election complete, creating user root..."
if ! mongosh --host "$mongo_host" --quiet --eval '
const adminDB = db.getSiblingDB("admin");
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
'; then
  echo "Failed to create admin user"
  exit 1
fi
