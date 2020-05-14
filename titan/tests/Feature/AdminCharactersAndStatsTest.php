<?php
namespace Titan\Tests\Feature;

use Illuminate\Foundation\Testing\RefreshDatabase;
use Titan\Tests\TestCase;

class AdminCharactersAndStatsTest extends TestCase {
    use RefreshDatabase;

    public function testPageLoads() {
        $this->get('/')
            ->assertStatus(200);

    }
}