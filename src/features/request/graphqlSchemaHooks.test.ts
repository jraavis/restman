//! Tests for `schemaFromIntrospectionResponse`.
//!
//! NOTE: `npx vitest run` cannot start in this sandbox (`ERR_REQUIRE_ESM` from
//! `html-encoding-sniffer`/`@exodus/bytes`, pre-existing in `node_modules`,
//! unrelated to this task; see PLAN.md "How to resume in a new session"), so
//! this file itself hasn't run under vitest. Unlike other hand-traced test
//! files in this repo, these exact assertions *were* run standalone via
//! `npx tsx` against a real introspection JSON payload (built with
//! `graphql`'s own `buildSchema`/`introspectionFromSchema`, not hand-written)
//! before this file was committed — see the GraphQL feature's task notes.

import { buildSchema, introspectionFromSchema } from "graphql";
import { describe, expect, it } from "vitest";
import { schemaFromIntrospectionResponse } from "./graphqlSchemaHooks";

const TOY_SDL = `
  type Pet {
    id: ID!
    name: String!
    owner: Owner
  }
  type Owner {
    name: String!
    pets: [Pet!]!
  }
  type Query {
    pets(limit: Int): [Pet!]!
  }
  type Mutation {
    addPet(name: String!): Pet!
  }
`;

function toyIntrospectionResponse(): string {
  const schema = buildSchema(TOY_SDL);
  return JSON.stringify({ data: introspectionFromSchema(schema) });
}

describe("schemaFromIntrospectionResponse", () => {
  it("reconstructs a GraphQLSchema whose types/fields match the source SDL", async () => {
    const schema = await schemaFromIntrospectionResponse(toyIntrospectionResponse());

    expect(schema.getQueryType()?.name).toBe("Query");
    expect(schema.getMutationType()?.name).toBe("Mutation");
    expect(schema.getType("Pet")).not.toBeNull();

    const petsField = schema.getQueryType()?.getFields()["pets"];
    expect(petsField?.type.toString()).toBe("[Pet!]!");
  });

  it("throws with the server's message on a GraphQL errors response", async () => {
    const body = JSON.stringify({ errors: [{ message: "nope" }] });
    await expect(schemaFromIntrospectionResponse(body)).rejects.toThrow(/nope/);
  });

  it("throws on a response with no __schema field", async () => {
    await expect(schemaFromIntrospectionResponse(JSON.stringify({ data: {} }))).rejects.toThrow(/__schema/);
  });
});
