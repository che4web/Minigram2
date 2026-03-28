env "local" {
  url = getenv("MINIGRAM_POSTGRES_URL")

  migration {
    dir = "file://migrations"
  }
}
